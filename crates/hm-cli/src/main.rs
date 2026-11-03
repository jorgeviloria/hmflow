//! CLI de HidroMac Flow.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hm_core::{Diagnostic, Diagnostics};
use hm_design::fluid::{water_kinematic_viscosity, water_vapor_pressure};
use hm_design::geometry::geometry;
use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics, mechanics_diagnostics};
use hm_design::{
    DEFAULT_K_EFF, DEFAULT_VOLUTE_C, DesignInput, DesignOptions, DesignReport, DesignResult,
    LEGACY_METHODS, METHOD, PARAM_SPECS, ParamSpec, WATER_DENSITY_RANGE_C,
    default_mechanical_efficiency, design, design_with_options, designed_si, param_spec,
    performance_curve, stored_si_matches, water_density,
};
use hm_design::{ViscousCorrection, viscous_correction, viscous_diagnostics};
use hm_formula::{Registry, eval, infer, parse};
use hm_project::{Param, Project, PumpType, Suction};
use hm_units::{Dimension, Quantity, parse_quantity, parse_unit};

#[derive(Parser)]
#[command(
    name = "hmflow-cli",
    version,
    about = "HidroMac Flow — diseño de bombas centrífugas"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// Se construye una vez por ejecución: el tamaño de la variante `Design` no
// importa y una caja solo complicaría el `match`.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    /// Crea un proyecto nuevo.
    New {
        /// Nombre del proyecto.
        name: String,
        /// Tipo de bomba: end_suction, inline, ring_section, split_case, barrel.
        #[arg(long, default_value = "end_suction")]
        kind: String,
        /// Norma aplicable; por defecto ISO 2858 solo para end-suction.
        #[arg(long)]
        standard: Option<String>,
    },
    /// Valida un proyecto.
    Check {
        /// Ruta del archivo .hmf.
        file: PathBuf,
    },
    /// Recalcula los parámetros automáticos de un proyecto end-suction a
    /// partir de su punto de operación y sus selecciones manuales.
    Recalc {
        /// Ruta del archivo .hmf.
        file: PathBuf,
        /// Vuelve a calcular las selecciones que dependen de nq (psi y phi
        /// según Gülich, beta2 por Euler, Z y el ojo: c0, cm0 y J),
        /// salvo las que estén en locked.
        #[arg(long)]
        reselect: bool,
    },
    /// Escribe el informe del diseño en JSON (formato hmflow.design-report):
    /// valores en SI, selecciones, secciones de voluta, curva y diagnósticos.
    Report {
        /// Ruta del archivo .hmf.
        file: PathBuf,
        /// Archivo JSON de salida; sin él, la salida estándar.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Sobrescribe el archivo de salida si ya existe.
        #[arg(long)]
        force: bool,
        /// Número de puntos de la curva.
        #[arg(long, default_value_t = 11)]
        curve_points: usize,
    },
    /// Lista las fórmulas disponibles.
    Formulas,
    /// Evalúa una expresión con unidades.
    Eval {
        /// Expresión, por ejemplo "Q/(pi*d2*b2)". Puede empezar por `-`.
        #[arg(allow_hyphen_values = true)]
        expr: String,
        /// Variable en formato nombre=valor, por ejemplo Q="120 m3/h".
        #[arg(long = "var", value_name = "NOMBRE=VALOR", allow_hyphen_values = true)]
        vars: Vec<String>,
    },
    /// Diseña preliminarmente una bomba end-suction OH1/OH2.
    Design {
        /// Nombre del proyecto y del archivo .hmf.
        #[arg(long, default_value = "diseno-end-suction")]
        name: String,
        /// Caudal de diseño, por ejemplo "120 m3/h".
        #[arg(long, default_value = "120 m3/h", allow_hyphen_values = true)]
        q: String,
        /// Altura de diseño, por ejemplo "45 m".
        #[arg(long, default_value = "45 m", allow_hyphen_values = true)]
        h: String,
        /// Velocidad de giro, por ejemplo "2900 rpm".
        #[arg(long, default_value = "2900 rpm", allow_hyphen_values = true)]
        n: String,
        /// Densidad del fluido, por ejemplo "998.2 kg/m3". Sin ella se usa la
        /// del agua a la temperatura indicada (Kell 1975, 0–150 °C).
        #[arg(long, allow_hyphen_values = true)]
        rho: Option<String>,
        /// Fluido; por defecto `water` si la densidad está en el rango del agua
        /// líquida (≈917..1000 kg/m3) y `custom` si no. Con `water` se comprueba
        /// que la densidad corresponda a la temperatura.
        #[arg(long)]
        fluid: Option<String>,
        /// Temperatura del fluido.
        #[arg(long, default_value = "20 degC", allow_hyphen_values = true)]
        temperature: String,
        /// Rendimiento mecánico de cojinetes y cierre, sin el rozamiento de
        /// discos, mayor que 0 y menor que 1. Sin él se usa la estimación de
        /// Gülich (ec. 3.10.14) para el caudal y la velocidad del punto.
        #[arg(long)]
        eta_m: Option<f64>,
        /// NPSH disponible de la instalación, por ejemplo "10 m". Sin él (ni
        /// --suction-pressure) no se evalúa el margen de cavitación.
        #[arg(long, allow_hyphen_values = true)]
        npsha: Option<String>,
        /// Presión absoluta sobre el líquido aspirado, por ejemplo "1 atm".
        /// Con ella el NPSHa se calcula de la instalación.
        #[arg(long, allow_hyphen_values = true, conflicts_with = "npsha")]
        suction_pressure: Option<String>,
        /// Altura del nivel del líquido sobre el eje; negativa si aspira
        /// desde abajo, por ejemplo "-3 m".
        #[arg(long, allow_hyphen_values = true, requires = "suction_pressure")]
        suction_height: Option<String>,
        /// Pérdidas de carga en la aspiración, por ejemplo "0.5 m".
        #[arg(long, allow_hyphen_values = true, requires = "suction_pressure")]
        suction_losses: Option<String>,
        /// Presión de vapor del fluido; sin ella, la del agua a la
        /// temperatura indicada (IAPWS-IF97).
        #[arg(long, allow_hyphen_values = true, requires = "suction_pressure")]
        vapor_pressure: Option<String>,
        /// Viscosidad del fluido, cinemática ("30 cSt") o dinámica ("25 cP");
        /// sin ella, la del agua a la temperatura indicada si es agua.
        #[arg(long, allow_hyphen_values = true)]
        viscosity: Option<String>,
        /// Archivo de salida; por defecto <name>.hmf.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Sobrescribe el archivo si ya existe.
        #[arg(long)]
        force: bool,
        /// Muestra las curvas preliminares H(Q), eta(Q), P(Q) y NPSHr(Q).
        #[arg(long)]
        curves: bool,
        /// Número de puntos de la curva.
        #[arg(long, default_value_t = 11)]
        curve_points: usize,
        /// Archivo CSV de curvas; implica --curves.
        #[arg(long)]
        curves_out: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Command::New {
            name,
            kind,
            standard,
        } => {
            if name.contains('/') || name.contains('\\') {
                return Err("el nombre no puede contener separadores de ruta".into());
            }

            let pump_type = PumpType::parse(&kind)
                .ok_or_else(|| format!("tipo de bomba desconocido: {kind}"))?;

            let path = PathBuf::from(format!("{name}.hmf"));
            if path.exists() {
                return Err(format!("ya existe {}", path.display()).into());
            }

            let mut project = Project::new(&name, pump_type);
            if let Some(standard) = standard {
                project.design.standard = standard;
            }
            project.save(&path)?;
            println!("creado {}", path.display());
            Ok(ExitCode::SUCCESS)
        }

        Command::Check { file } => {
            let project = Project::load(&file)?;
            let diagnostics = project.validate(&Registry::all());
            let design_diagnostics = if diagnostics.has_errors() {
                Diagnostics::new()
            } else {
                design_audit(&project)
            };

            let mut printed = false;
            for diagnostic in diagnostics.iter() {
                println!(
                    "[{:?}] {}: {}",
                    diagnostic.severity, diagnostic.code, diagnostic.message
                );
                if let Some(hint) = &diagnostic.hint {
                    println!("        sugerencia: {hint}");
                }
                printed = true;
            }
            for diagnostic in design_diagnostics.iter() {
                println!(
                    "[{:?}] {}: {}",
                    diagnostic.severity, diagnostic.code, diagnostic.message
                );
                if let Some(hint) = &diagnostic.hint {
                    println!("        sugerencia: {hint}");
                }
                printed = true;
            }
            if !printed {
                println!("{}: sin diagnósticos", file.display());
            }

            if diagnostics.has_errors() || design_diagnostics.has_errors() {
                Ok(ExitCode::FAILURE)
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }

        Command::Recalc { file, reselect } => recalc(&file, reselect),

        Command::Report {
            file,
            out,
            force,
            curve_points,
        } => {
            let report = design_report(&file, curve_points)?;
            let json = serde_json::to_string_pretty(&report)?;
            match out {
                Some(path) => {
                    if path.exists() && !force {
                        return Err(format!(
                            "ya existe {}; usa --force para sobrescribir",
                            path.display()
                        )
                        .into());
                    }
                    std::fs::write(&path, json + "\n")?;
                    eprintln!("informe escrito en {}", path.display());
                }
                None => println!("{json}"),
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Formulas => {
            let registry = Registry::all();
            println!("{:<32} {:<10} {:<8} TÍTULO", "ID", "UNIDAD", "VAL.");
            for formula in registry.iter() {
                println!(
                    "{:<32} {:<10} {:<8} {}",
                    formula.id, formula.units, formula.validation, formula.title
                );
            }
            let (pilot, reference, bench) = registry.validation_counts();
            println!(
                "\n{} fórmulas ({pilot} piloto, {reference} referencia, {bench} banco)",
                registry.len()
            );
            Ok(ExitCode::SUCCESS)
        }

        Command::Eval { expr, vars } => {
            let mut environment: BTreeMap<String, Quantity> = BTreeMap::new();

            for pair in vars {
                let (name, value) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("formato esperado nombre=valor, llegó: {pair}"))?;
                environment.insert(name.trim().to_string(), parse_quantity(value.trim())?);
            }

            let expression = parse(&expr)?;
            let dimensions = environment
                .iter()
                .map(|(name, value)| (name.clone(), value.dim()))
                .collect();

            let inferred = infer(&expression, &dimensions)?;
            let result = eval(&expression, &environment)?;
            result.ensure_physical()?;

            println!(
                "{} {}  [{}]",
                result.value(),
                result.unit.symbol,
                inferred.symbol()
            );
            Ok(ExitCode::SUCCESS)
        }

        Command::Design {
            name,
            q,
            h,
            n,
            rho,
            fluid,
            temperature,
            eta_m,
            npsha,
            suction_pressure,
            suction_height,
            suction_losses,
            vapor_pressure,
            viscosity,
            out,
            force,
            curves,
            curve_points,
            curves_out,
        } => {
            if name.contains('/') || name.contains('\\') {
                return Err("el nombre no puede contener separadores de ruta".into());
            }

            let npsha_text = npsha.clone();
            let temperature_k = parse_temperature(&temperature)?;
            let rho_text = match rho {
                Some(text) => text,
                None => {
                    if fluid
                        .as_deref()
                        .is_some_and(|fluid| !fluid.eq_ignore_ascii_case("water"))
                    {
                        return Err("indica --rho: solo se conoce la densidad del agua".into());
                    }
                    let density = water_density(temperature_k).ok_or(
                        "la temperatura está fuera de 0..150 °C, el intervalo de la densidad del agua; indica --rho",
                    )?;
                    format!("{density:.3} kg/m3")
                }
            };
            let rho_quantity = parse_quantity(&rho_text)?;
            let fluid = fluid.unwrap_or_else(|| {
                if looks_like_water(rho_quantity.si) {
                    "water".to_string()
                } else {
                    "custom".to_string()
                }
            });
            let suction = suction_pressure.map(|pressure| Suction {
                pressure,
                height: suction_height.unwrap_or_default(),
                losses: suction_losses.unwrap_or_default(),
                vapor_pressure: vapor_pressure.unwrap_or_default(),
            });
            let npsha_quantity = match &suction {
                Some(suction) => Some(suction_npsha(
                    suction,
                    &fluid,
                    &temperature,
                    rho_quantity.si,
                )?),
                None => npsha.as_deref().map(parse_quantity).transpose()?,
            };
            let q_quantity = parse_quantity(&q)?;
            let n_quantity = parse_quantity(&n)?;
            let eta_m = match eta_m {
                Some(eta_m) => eta_m,
                None => default_mechanical_efficiency(&q_quantity, &n_quantity)?,
            };
            let viscosity_text = viscosity.unwrap_or_default();
            let nu = kinematic_viscosity(&viscosity_text, &fluid, &temperature, rho_quantity.si)?;
            let input = DesignInput {
                q: q_quantity,
                h: parse_quantity(&h)?,
                n: n_quantity,
                rho: rho_quantity,
                eta_m,
                npsha: npsha_quantity,
                nu,
            };

            if !(2..=1000).contains(&curve_points) {
                return Err("--curve-points debe estar entre 2 y 1000".into());
            }

            let result = design(&input)?;

            println!("Diseño preliminar end-suction OH1/OH2");
            println!("  Punto: Q = {q}, H = {h}, n = {n}");
            println!(
                "  nq = {:.2}   psi = {:.3}   phi = {:.3}   beta2 = {:.1} deg   Z = {}",
                result.nq,
                result.selection.psi,
                result.selection.phi,
                result.selection.beta2_deg,
                result.selection.blade_count
            );
            println!(
                "  D0 = {:.4} m   D1 = {:.4} m   B1 = {:.4} m",
                result.d0.value(),
                result.d1.value(),
                result.b1.value()
            );
            println!(
                "  D2 = {:.4} m   B2 = {:.4} m   u2 = {:.2} m/s",
                result.d2.value(),
                result.b2.value(),
                result.u2.value()
            );
            println!(
                "  eta_h = {:.3}   eta_v = {:.3}   eta = {:.3}",
                result.eta_h, result.eta_v, result.eta
            );
            println!(
                "  P_hyd = {:.0} W   P_eje = {:.0} W   P_max = {:.0} W",
                result.p_hyd.value(),
                result.p_shaft.value(),
                result.p_max.value()
            );
            println!(
                "  T = {:.2} N*m   T_max = {:.2} N*m",
                result.torque.value(),
                result.torque_max.value()
            );
            println!(
                "  NPSHr_d = {:.2} m   H0 = {:.2} m   Hth = 0 en {:.2}·Qd   pendiente H/Hd en Qd {:.3}",
                result.npshr_design.value(),
                result.shutoff_head.value(),
                result.zero_head_flow_ratio,
                result.head_slope
            );
            println!(
                "  Voluta: R3 = {:.4} m   B3 = {:.4} m   C3 = {:.2} m/s",
                result.volute.base_radius.value(),
                result.volute.width.value(),
                result.volute.inlet_velocity.value()
            );
            println!(
                "  A360 = {:.6} m2   D360 = {:.4} m   R4 = {:.4} m",
                result.volute.throat_area.value(),
                result.volute.throat_diameter.value(),
                result.volute.outer_radius.value()
            );
            println!(
                "  Pérdidas: hyd = {:.0} W   vol = {:.0} W   discos = {:.0} W   mech = {:.0} W",
                result.p_hyd_loss.value(),
                result.p_vol_loss.value(),
                result.p_disk_loss.value(),
                result.p_mech_loss.value()
            );
            println!(
                "  NPSH3: entrada sin choque {:.2}·Qd   voluta {:.2}·Qd   subida desde {:.2}·Qd   recirculación ≈ {:.2}·Qd",
                result.shockless_flow.si / input.q.si,
                result.volute_matching_flow.si / input.q.si,
                result.transition_ratio,
                result.recirculation_onset
            );
            println!(
                "  Contraste Gülich: ηv anillo delantero {:.3} (Qsp/Q {:.3}; ηv del método {:.3})   ηm {:.3} (motor {:.3})",
                result.seal_eta_v,
                result.seal_leakage_ratio,
                result.eta_v,
                result.mechanical_eta_estimate,
                input.eta_m
            );
            println!(
                "  Pérdidas Gülich: discos PRR/Pu {:.3}   ηh estadístico {:.3} (dimensionado {:.3})   ηv fugas {:.3}   ηm {:.3}   → η {:.3}",
                result.disk_friction_ratio,
                result.statistical_eta_h,
                result.eta_h,
                result.leakage_eta_v,
                input.eta_m,
                result.eta
            );
            let shape = geometry(&input, &result)?;
            println!(
                "  Álabe: β1B = {:.1} deg   envolvimiento = {:.1} deg",
                shape.impeller.beta1b.to_degrees(),
                shape.impeller.wrap_angle.to_degrees()
            );
            println!(
                "  Salida: lengua dz/d2 = {:.3}   DN {:.0}   difusor ≥ {:.3} m ({:.1} deg)",
                2.0 * shape.outlet.cutwater_radius / result.d2.si,
                shape.outlet.nominal_diameter * 1000.0,
                shape.outlet.min_length,
                shape.outlet.half_angle.to_degrees()
            );
            let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS)?;
            println!(
                "  Eje: ds ≥ {:.1} mm (torsión, τ = {:.1} MPa)   cubo ≈ {:.1} mm",
                mech.shaft_min_diameter * 1000.0,
                mech.shaft_shear_stress / 1e6,
                mech.hub_diameter * 1000.0
            );
            println!(
                "  Empuje radial: {:.0} N cerrada   {:.0} N diseño   {:.0} N a {:.1}·Qd   axial ≈ {:.0}..{:.0} N",
                mech.radial_thrust_shutoff,
                mech.radial_thrust_design,
                mech.radial_thrust_overload,
                hm_design::PREFERRED_OPERATING_REGION.1,
                mech.axial_thrust.0,
                mech.axial_thrust.1
            );
            println!(
                "  Espesores: álabe e = {:.1} mm   salida {:.1} mm   tapas {:.1} mm   holgura anillo Δd = {:.2} mm",
                mech.blade_thickness * 1000.0,
                mech.trailing_edge_thickness * 1000.0,
                mech.shroud_thickness * 1000.0,
                mech.seal_clearance * 1000.0
            );
            match mech.motor_rating {
                Some(rating) => println!(
                    "  Motor: {:.2} kW × {:.2} (API 610) = {:.2} kW → {} kW (IEC)   máx. curva {:.2} kW",
                    mech.rated_power / 1000.0,
                    mech.driver_margin,
                    mech.driver_power / 1000.0,
                    rating / 1000.0,
                    mech.curve_max_power / 1000.0
                ),
                None => println!(
                    "  Motor: {:.2} kW × {:.2} (API 610) = {:.2} kW, fuera de la serie IEC",
                    mech.rated_power / 1000.0,
                    mech.driver_margin,
                    mech.driver_power / 1000.0
                ),
            }
            if let (Some(npsha), Some(_)) = (&input.npsha, &suction) {
                println!("  NPSHa de la instalación = {:.2} m", npsha.si);
            }

            let mut shown = result.diagnostics.clone();
            shown.extend(fluid_diagnostics(&fluid, &temperature, input.rho.si).into_vec());
            shown.extend(npsh_fluid_diagnostics(&fluid, input.npsha.is_some()).into_vec());
            let (viscous_diagnostics, correction) =
                viscosity_effect(&fluid, &temperature, &viscosity_text, &input, &result)?;
            if let Some(correction) = &correction {
                println!(
                    "  Viscosidad: ν = {:.3} cSt   fQ = {:.3}   fH = {:.3}   fη = {:.3}",
                    correction.nu * 1e6,
                    correction.f_q,
                    correction.f_h,
                    correction.f_eta
                );
            }
            shown.extend(viscous_diagnostics.into_vec());
            shown.extend(mechanics_diagnostics(&mech, &result).into_vec());
            print_diagnostics(&shown);

            if shown.has_errors() {
                return Err("el diseño tiene errores; no se escribió el proyecto".into());
            }

            let path = out.unwrap_or_else(|| PathBuf::from(format!("{name}.hmf")));
            let curve_path = curves_out;
            let curve = if curves || curve_path.is_some() {
                Some(performance_curve(&input, &result, curve_points))
            } else {
                None
            };

            if path.exists() && !force {
                return Err(format!(
                    "ya existe {}; usa --force para sobrescribir",
                    path.display()
                )
                .into());
            }
            if let Some(curve_path) = &curve_path
                && curve_path.exists()
                && !force
            {
                return Err(format!(
                    "ya existe {}; usa --force para sobrescribir",
                    curve_path.display()
                )
                .into());
            }

            let mut project = Project::new(&name, PumpType::EndSuction);
            project.operating.q = q;
            project.operating.h = h;
            project.operating.n = n;
            project.operating.fluid = fluid;
            project.operating.temperature = temperature;
            project.operating.viscosity = viscosity_text;
            if suction.is_some() {
                project.suction = suction;
            } else {
                project.operating.npsha = npsha_text.unwrap_or_default();
            }
            project.design.method = METHOD.to_string();

            let defaults = ManualDefaults {
                rho: rho_text,
                eta_m: eta_m.to_string(),
            };
            project.params = design_params(&result, None, &defaults)?.0;

            let diagnostics = project.validate(&Registry::all());
            if diagnostics.has_errors() {
                for diagnostic in diagnostics.iter() {
                    eprintln!(
                        "[{:?}] {}: {}",
                        diagnostic.severity, diagnostic.code, diagnostic.message
                    );
                }
                return Err("el proyecto generado no valida; no se escribió".into());
            }

            project.save(&path)?;
            println!("escrito {}", path.display());

            if let Some(curve) = &curve {
                println!("\n  Q (m3/h)   H (m)    eta     P_eje (kW)  NPSHr (m)");
                for point in curve {
                    println!(
                        "  {:8.2}   {:6.3}   {:5.3}   {:9.3}   {:7.3}",
                        point.q.value(),
                        point.h.value(),
                        point.eta,
                        point.p_shaft.value(),
                        point.npshr.value()
                    );
                }

                if let Some(curve_path) = &curve_path {
                    let mut csv = String::from("Q_m3h,H_m,eta,P_shaft_kW,NPSHr_m\n");
                    for point in curve {
                        csv.push_str(&format!(
                            "{:.3},{:.4},{:.4},{:.4},{:.4}\n",
                            point.q.value(),
                            point.h.value(),
                            point.eta,
                            point.p_shaft.value(),
                            point.npshr.value()
                        ));
                    }
                    std::fs::write(curve_path, csv)?;
                    println!("curvas escritas en {}", curve_path.display());
                }
            }

            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Entrada y selecciones del diseño tal como las guarda el proyecto.
fn design_inputs(
    project: &Project,
) -> Result<(DesignInput, DesignOptions), Box<dyn std::error::Error>> {
    let q = parse_quantity(&project.operating.q)?;
    let h = parse_quantity(&project.operating.h)?;
    let n = parse_quantity(&project.operating.n)?;
    let rho_unit = parse_unit("kg/m3")?;
    let rho = Quantity::from_si(
        param_f64(project, "rho", "kg/m3")?.unwrap_or_else(|| default_density(project)),
        rho_unit,
    );
    let eta_m = match param_f64(project, "eta_m", "1")? {
        Some(eta_m) => eta_m,
        None => default_mechanical_efficiency(&q, &n)?,
    };
    let npsha = match &project.suction {
        Some(suction) => Some(suction_npsha(
            suction,
            &project.operating.fluid,
            &project.operating.temperature,
            rho.si,
        )?),
        None if project.operating.npsha.trim().is_empty() => None,
        None => Some(parse_quantity(&project.operating.npsha)?),
    };
    let options = design_options(project)?;
    let nu = kinematic_viscosity(
        &project.operating.viscosity,
        &project.operating.fluid,
        &project.operating.temperature,
        rho.si,
    )?;

    Ok((
        DesignInput {
            q,
            h,
            n,
            rho,
            eta_m,
            npsha,
            nu,
        },
        options,
    ))
}

fn recompute_design_diagnostics(
    project: &Project,
) -> Result<Option<hm_core::Diagnostics>, Box<dyn std::error::Error>> {
    if project.design.pump_type != PumpType::EndSuction {
        return Ok(None);
    }

    let (input, options) = design_inputs(project)?;
    let result = design_with_options(&input, &options)?;
    let mut diagnostics = result.diagnostics.clone();
    add_fluid_temperature_diagnostics(project, &input, &mut diagnostics);
    diagnostics.extend(
        project_viscosity_effect(project, &input, &result)?
            .0
            .into_vec(),
    );
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS)?;
    diagnostics.extend(mechanics_diagnostics(&mech, &result).into_vec());
    add_selection_diagnostics(project, &result, &mut diagnostics);
    compare_stored_autos(&result, project, &mut diagnostics);
    Ok(Some(diagnostics))
}

/// Selecciones que `recalc --reselect` vuelve a tomar del motor para el punto.
///
/// J entra porque el ojo automático lo fija junto con c0 y cm0. Si se deja
/// el J de otro punto, el motor no minimiza el NPSH y c0 queda distinto.
const NQ_SELECTIONS: [&str; 7] = ["psi", "phi", "beta2", "blade_count", "c0", "cm0", "J"];

/// Valor, en SI, de la selección que el motor haría para este punto.
///
/// β2 no se compara: sale de Euler con el Z del proyecto. Si no cuadra con
/// la altura, lo señala `HM-DES-019`.
fn fresh_selection_si(result: &DesignResult, name: &str) -> Option<f64> {
    Some(match name {
        "psi" => result.selection.psi,
        "phi" => result.selection.phi,
        "blade_count" => f64::from(result.selection.blade_count),
        "c0" => result.selection.c0,
        "cm0" => result.selection.cm0,
        "J" => result.selection.j,
        _ => return None,
    })
}

/// Selecciones `manual` que ya no coinciden con lo que el motor elegiría
/// para el punto actual, con el fluido del proyecto y sin otras selecciones
/// manuales. Las `locked` son decisiones explícitas y no se señalan.
fn stale_selections(project: &Project) -> Vec<&'static str> {
    let Ok((input, _)) = design_inputs(project) else {
        return Vec::new();
    };
    let Ok(fresh) = design(&input) else {
        return Vec::new();
    };
    NQ_SELECTIONS
        .into_iter()
        .filter(|name| {
            let Some(Param::Manual { value }) = project.params.get(*name) else {
                return false;
            };
            let (Ok(quantity), Some(expected)) =
                (parse_quantity(value), fresh_selection_si(&fresh, name))
            else {
                return false;
            };
            !stored_si_matches(expected, quantity.si)
        })
        .collect()
}

fn add_selection_diagnostics(
    project: &Project,
    result: &DesignResult,
    diagnostics: &mut Diagnostics,
) {
    let stale = stale_selections(project);
    if stale.is_empty() {
        return;
    }
    let names = stale
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    diagnostics.push(
        Diagnostic::warning(
            "HM-DES-013",
            format!(
                "{names} no {} con la selección del motor para nq = {:.2}; puede venir de otro punto de operación",
                if stale.len() == 1 { "coincide" } else { "coinciden" },
                result.nq
            ),
        )
        .with_hint(
            "Usa `hmflow-cli recalc --reselect <archivo>` para volver a la selección \
             del motor en este punto, o márcalas como locked si son decisiones tuyas.",
        ),
    );
}

/// Recalcula los `auto` de un proyecto end-suction y lo reescribe.
///
/// Conserva los parámetros manuales y bloqueados; los que falten se rellenan
/// con la selección del motor y se informa de cada uno. Con `reselect`, las
/// selecciones `manual` que dependen del punto se vuelven a tomar del motor.
fn recalc(file: &std::path::Path, reselect: bool) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut project = Project::load(file)?;

    let diagnostics = project.validate(&Registry::all());
    if diagnostics.has_errors() {
        print_diagnostics(&diagnostics);
        return Err("el proyecto tiene errores; no se recalculó".into());
    }
    if project.design.pump_type != PumpType::EndSuction {
        return Err(format!(
            "recalc solo admite end-suction; el proyecto es {}",
            project.design.pump_type
        )
        .into());
    }
    let legacy = LEGACY_METHODS.contains(&project.design.method.as_str());
    if !project.design.method.is_empty() && project.design.method != METHOD && !legacy {
        return Err(format!("método de diseño desconocido: {}", project.design.method).into());
    }
    let migrated_from = legacy.then(|| project.design.method.clone());
    if let Some(name) = project
        .params
        .keys()
        .find(|name| param_spec(name).is_none())
    {
        return Err(
            format!("parámetro desconocido: `{name}`; corrígelo antes de recalcular").into(),
        );
    }

    // Hasta end_suction_v0.3, ηm incluía el rozamiento de discos; desde la
    // v0.4 va aparte, así que el ηm guardado ya no significa lo mismo.
    let lumped_eta_m = matches!(
        migrated_from.as_deref(),
        Some("end_suction_v0.1" | "end_suction_v0.2" | "end_suction_v0.3")
    );
    let old_eta_m = if lumped_eta_m {
        project
            .params
            .remove("eta_m")
            .as_ref()
            .and_then(param_value)
            .map(str::to_string)
    } else {
        None
    };

    let mut reselected = Vec::new();
    if reselect {
        for name in NQ_SELECTIONS {
            if let Some(Param::Manual { value }) = project.params.get(name) {
                reselected.push((name, value.clone()));
                project.params.remove(name);
            }
        }
    }

    let (input, options) = design_inputs(&project)?;
    let result = design_with_options(&input, &options)?;
    let mut diagnostics = result.diagnostics.clone();
    add_fluid_temperature_diagnostics(&project, &input, &mut diagnostics);
    diagnostics.extend(
        project_viscosity_effect(&project, &input, &result)?
            .0
            .into_vec(),
    );
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS)?;
    diagnostics.extend(mechanics_diagnostics(&mech, &result).into_vec());
    add_selection_diagnostics(&project, &result, &mut diagnostics);
    print_diagnostics(&diagnostics);
    if diagnostics.has_errors() {
        return Err("el diseño tiene errores; no se escribió el proyecto".into());
    }

    let defaults = ManualDefaults {
        rho: format!("{} kg/m3", input.rho.si),
        eta_m: input.eta_m.to_string(),
    };
    let (params, filled) = design_params(&result, Some(&project.params), &defaults)?;
    project.params = params;
    project.design.method = METHOD.to_string();

    let diagnostics = project.validate(&Registry::all());
    if diagnostics.has_errors() {
        print_diagnostics(&diagnostics);
        return Err("el proyecto recalculado no valida; no se escribió".into());
    }

    project.save(file)?;
    for (name, old) in &reselected {
        let value = project
            .params
            .get(*name)
            .and_then(param_value)
            .unwrap_or("");
        if old != value {
            println!(
                "  `{name}`: {old} → {value} (selección por defecto para nq = {:.2})",
                result.nq
            );
        }
    }
    for name in filled
        .iter()
        .filter(|name| !reselected.iter().any(|(old, _)| old == name))
        .filter(|name| !(old_eta_m.is_some() && name.as_str() == "eta_m"))
    {
        let value = project.params.get(name).and_then(param_value).unwrap_or("");
        println!("  `{name}` no estaba en el proyecto; se usó {value}");
    }
    if let Some(old) = migrated_from {
        if let Some(old_eta_m) = old_eta_m {
            println!(
                "  `eta_m`: {old_eta_m} → {} (desde end_suction_v0.4 no incluye el rozamiento de discos)",
                input.eta_m
            );
        }
        println!("  método migrado de {old} a {METHOD}");
    }
    println!("recalculado {}", file.display());
    Ok(ExitCode::SUCCESS)
}

/// Informe del diseño de un proyecto end-suction con el método actual.
///
/// Lleva los diagnósticos de la validación del proyecto y de la auditoría del
/// diseño. Un proyecto con errores de formato o de un método anterior no
/// tiene informe: primero hay que corregirlo o migrarlo.
fn design_report(
    file: &std::path::Path,
    curve_points: usize,
) -> Result<DesignReport, Box<dyn std::error::Error>> {
    if !(2..=1000).contains(&curve_points) {
        return Err("--curve-points debe estar entre 2 y 1000".into());
    }
    let project = Project::load(file)?;
    let validation = project.validate(&Registry::all());
    if validation.has_errors() {
        print_diagnostics(&validation);
        return Err("el proyecto tiene errores; no hay informe".into());
    }
    if project.design.pump_type != PumpType::EndSuction {
        return Err(format!(
            "solo hay informe de end-suction; el proyecto es {}",
            project.design.pump_type
        )
        .into());
    }
    if LEGACY_METHODS.contains(&project.design.method.as_str()) {
        return Err(format!(
            "el proyecto es de {}; migra con `hmflow-cli recalc` antes de pedir el informe",
            project.design.method
        )
        .into());
    }
    if !project.design.method.is_empty() && project.design.method != METHOD {
        return Err(format!("método de diseño desconocido: {}", project.design.method).into());
    }

    let (input, options) = design_inputs(&project)?;
    let result = design_with_options(&input, &options)?;
    let mut diagnostics = validation.into_vec();
    diagnostics.extend(design_audit(&project).into_vec());
    let report = DesignReport::new(&input, &result, curve_points, diagnostics)
        .with_geometry(&geometry(&input, &result)?)
        .with_mechanics(&mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS)?);
    Ok(
        match project_viscosity_effect(&project, &input, &result)?.1 {
            Some(correction) => report.with_viscous(&correction),
            None => report,
        },
    )
}

/// NPSHa de una instalación de aspiración con la fórmula `npsh.npsha`.
///
/// La presión de vapor es la de `[suction]` o, si falta y el fluido es agua,
/// la de IAPWS-IF97 a la temperatura del proyecto (20 °C si no se indica).
fn suction_npsha(
    suction: &Suction,
    fluid: &str,
    temperature: &str,
    rho: f64,
) -> Result<Quantity, Box<dyn std::error::Error>> {
    let text_or_zero = |text: &str| -> Result<Quantity, Box<dyn std::error::Error>> {
        Ok(if text.trim().is_empty() {
            parse_quantity("0 m")?
        } else {
            parse_quantity(text)?
        })
    };
    let pressure = parse_quantity(&suction.pressure)?;
    let vapor = if suction.vapor_pressure.trim().is_empty() {
        let water = fluid.is_empty() || fluid.eq_ignore_ascii_case("water");
        if !water {
            return Err(format!(
                "el fluido es `{fluid}`: indica la presión de vapor de la aspiración"
            )
            .into());
        }
        let temperature_k = if temperature.trim().is_empty() {
            293.15
        } else {
            parse_temperature(temperature)?
        };
        let pascal = water_vapor_pressure(temperature_k)
            .ok_or("la temperatura está fuera del intervalo de la presión de vapor del agua")?;
        Quantity::from_si(pascal, parse_unit("Pa")?)
    } else {
        parse_quantity(&suction.vapor_pressure)?
    };
    let inputs: BTreeMap<String, Quantity> = [
        ("p_s", pressure),
        ("p_v", vapor),
        ("rho", Quantity::from_si(rho, parse_unit("kg/m3")?)),
        ("z", text_or_zero(&suction.height)?),
        ("hf", text_or_zero(&suction.losses)?),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value))
    .collect();
    Ok(Registry::all().evaluate("npsh.npsha", &inputs)?)
}

/// Viscosidad cinemática del fluido en m²/s: la indicada (cinemática, o
/// dinámica dividida por ρ) o la del agua a la temperatura del proyecto.
/// `None` si el fluido no es agua y no se indicó, o si la temperatura cae
/// fuera de la correlación del agua.
fn kinematic_viscosity(
    viscosity: &str,
    fluid: &str,
    temperature: &str,
    rho: f64,
) -> Result<Option<f64>, Box<dyn std::error::Error>> {
    if !viscosity.trim().is_empty() {
        let quantity = parse_quantity(viscosity)?;
        if quantity.dim() == parse_unit("m2/s")?.dim {
            return Ok(Some(quantity.si));
        }
        if quantity.dim() == parse_unit("Pa*s")?.dim {
            return Ok(Some(quantity.si / rho));
        }
        return Err(format!("la viscosidad debe ser m2/s (cSt) o Pa*s (cP): {viscosity}").into());
    }
    let water = fluid.is_empty() || fluid.eq_ignore_ascii_case("water");
    if !water {
        return Ok(None);
    }
    let temperature_k = if temperature.trim().is_empty() {
        293.15
    } else {
        parse_temperature(temperature)?
    };
    Ok(water_kinematic_viscosity(temperature_k))
}

/// Corrección por viscosidad y sus avisos. Sin viscosidad conocida de un
/// fluido que no es agua, informa de que no se evaluó (`HM-DES-022`).
fn viscosity_effect(
    fluid: &str,
    temperature: &str,
    viscosity: &str,
    input: &DesignInput,
    result: &DesignResult,
) -> Result<(Diagnostics, Option<ViscousCorrection>), Box<dyn std::error::Error>> {
    let mut diagnostics = Diagnostics::new();
    match kinematic_viscosity(viscosity, fluid, temperature, input.rho.si)? {
        Some(nu) => {
            let correction = viscous_correction(input, result, nu)?;
            diagnostics.extend(viscous_diagnostics(&correction, result.nq).into_vec());
            Ok((diagnostics, Some(correction)))
        }
        None => {
            let water = fluid.is_empty() || fluid.eq_ignore_ascii_case("water");
            if !water {
                diagnostics.push(
                    Diagnostic::info(
                        "HM-DES-022",
                        format!(
                            "el fluido es `{fluid}` y no tiene viscosidad: no se evaluó su efecto"
                        ),
                    )
                    .with_hint("Indica la viscosidad en [operating] (viscosity = \"30 cSt\")."),
                );
            }
            Ok((diagnostics, None))
        }
    }
}

fn project_viscosity_effect(
    project: &Project,
    input: &DesignInput,
    result: &DesignResult,
) -> Result<(Diagnostics, Option<ViscousCorrection>), Box<dyn std::error::Error>> {
    viscosity_effect(
        &project.operating.fluid,
        &project.operating.temperature,
        &project.operating.viscosity,
        input,
        result,
    )
}

fn print_diagnostics(diagnostics: &Diagnostics) {
    for diagnostic in diagnostics.iter() {
        println!(
            "[{:?}] {}: {}",
            diagnostic.severity, diagnostic.code, diagnostic.message
        );
        if let Some(hint) = &diagnostic.hint {
            println!("        sugerencia: {hint}");
        }
    }
}

/// Texto de los parámetros manuales que no salen de la selección del motor.
struct ManualDefaults {
    rho: String,
    eta_m: String,
}

/// Valor manual que usó el motor para `name`.
fn default_manual_text(
    name: &str,
    result: &DesignResult,
    defaults: &ManualDefaults,
) -> Option<String> {
    let selection = &result.selection;
    Some(match name {
        "rho" => defaults.rho.clone(),
        "eta_m" => defaults.eta_m.clone(),
        "mm" => "0.001 m".to_string(),
        "psi" => selection.psi.to_string(),
        "phi" => selection.phi.to_string(),
        "beta2" => format!("{} deg", selection.beta2_deg),
        "blade_count" => selection.blade_count.to_string(),
        "c0" => format!("{} m/s", selection.c0),
        "cm0" => format!("{} m/s", selection.cm0),
        "A" => selection.a.to_string(),
        "J" => selection.j.to_string(),
        "margin" => selection.margin.to_string(),
        "k3" => (result.volute.base_radius.si / (result.d2.si / 2.0)).to_string(),
        "volute_c" => DEFAULT_VOLUTE_C.to_string(),
        "k_eff" => DEFAULT_K_EFF.to_string(),
        _ => return None,
    })
}

/// Valor calculado de un parámetro `auto`, en la unidad de su especificación.
fn auto_text(
    result: &DesignResult,
    spec: &ParamSpec,
) -> Result<String, Box<dyn std::error::Error>> {
    let si = designed_si(result, spec.name)
        .ok_or_else(|| format!("el diseño no calcula `{}`", spec.name))?;
    if spec.unit == "1" {
        return Ok(si.to_string());
    }
    let value = Quantity::from_si(si, parse_unit(spec.unit)?).value();
    Ok(format!("{value} {}", spec.unit))
}

/// Parámetros generados y nombres de los manuales que se rellenaron.
type GeneratedParams = (BTreeMap<String, Param>, Vec<String>);

/// Parámetros del método [`METHOD`] para `result`.
///
/// Los `auto` se escriben siempre con el valor recalculado. Los manuales y
/// bloqueados de `existing` se conservan; los que faltan se rellenan con el
/// valor que usó el motor y se devuelven en la segunda lista.
fn design_params(
    result: &DesignResult,
    existing: Option<&BTreeMap<String, Param>>,
    defaults: &ManualDefaults,
) -> Result<GeneratedParams, Box<dyn std::error::Error>> {
    let mut params = BTreeMap::new();
    let mut filled = Vec::new();

    for spec in PARAM_SPECS {
        let param = if let Some(formula) = spec.formula {
            auto(formula, auto_text(result, spec)?)
        } else {
            match existing.and_then(|params| params.get(spec.name)) {
                Some(param @ (Param::Manual { .. } | Param::Locked { .. })) => param.clone(),
                Some(_) => {
                    return Err(format!(
                        "`{}` debe ser manual o locked para recalcular",
                        spec.name
                    )
                    .into());
                }
                None => {
                    let value = default_manual_text(spec.name, result, defaults)
                        .ok_or_else(|| format!("sin valor por defecto para `{}`", spec.name))?;
                    if existing.is_some() {
                        filled.push(spec.name.to_string());
                    }
                    manual(value)
                }
            }
        };
        params.insert(spec.name.to_string(), param);
    }

    Ok((params, filled))
}

fn design_options(project: &Project) -> Result<DesignOptions, Box<dyn std::error::Error>> {
    Ok(DesignOptions {
        psi: param_f64(project, "psi", "1")?,
        phi: param_f64(project, "phi", "1")?,
        beta2_rad: param_f64(project, "beta2", "deg")?,
        blade_count: param_u32(project, "blade_count")?,
        c0: param_f64(project, "c0", "m/s")?,
        cm0: param_f64(project, "cm0", "m/s")?,
        a: param_f64(project, "A", "1")?,
        j: param_f64(project, "J", "1")?,
        margin: param_f64(project, "margin", "1")?,
        k3: param_f64(project, "k3", "1")?,
        volute_width_factor: param_f64(project, "volute_c", "1")?,
        k_eff: param_f64(project, "k_eff", "1")?,
        // La holgura medida entrará como parámetro cuando se adopte.
        seal_clearance: None,
        // ηv, agujeros de equilibrio, razón de la garganta y rugosidad entran
        // como parámetros en la tarea 12.
        eta_v: None,
        balance_holes: false,
        volute_ratio: None,
        impeller_roughness: None,
        volute_channels: None,
    })
}

fn param_f64(
    project: &Project,
    name: &str,
    unit_text: &str,
) -> Result<Option<f64>, Box<dyn std::error::Error>> {
    let Some(param) = project.params.get(name) else {
        return Ok(None);
    };
    if matches!(param, Param::Override { .. }) {
        return Err(format!("`{name}` está en override; la auditoría no evalúa overrides").into());
    }
    let Some(value) = param_value(param) else {
        return Ok(None);
    };
    let quantity = parse_quantity(value)?;
    let expected = parse_unit(unit_text)?;
    if quantity.dim() != expected.dim {
        return Err(format!("`{name}` debe ser {unit_text}").into());
    }
    Ok(Some(quantity.si))
}

fn param_u32(project: &Project, name: &str) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let Some(value) = param_f64(project, name, "1")? else {
        return Ok(None);
    };
    if value.fract() != 0.0 || !(1.0..=100.0).contains(&value) {
        return Err(format!("`{name}` debe ser un entero entre 1 y 100").into());
    }
    Ok(Some(value as u32))
}

/// Diferencia relativa admitida entre `rho` y la densidad del agua.
const WATER_DENSITY_TOLERANCE: f64 = 0.005;

/// Densidad dentro del rango del agua líquida entre 0 y 150 °C.
///
/// Sirve para suponer el fluido cuando no se indica: una densidad de agua a
/// otra temperatura se trata como agua, para que `HM-DES-009` la señale.
fn looks_like_water(rho_si: f64) -> bool {
    // El agua es más densa a 4 °C y más ligera en el extremo caliente.
    let hottest = WATER_DENSITY_RANGE_C.1 + 273.15;
    let (Some(lightest), Some(densest)) = (water_density(hottest), water_density(277.15)) else {
        return false;
    };
    (lightest * (1.0 - WATER_DENSITY_TOLERANCE)..=densest * (1.0 + WATER_DENSITY_TOLERANCE))
        .contains(&rho_si)
}

/// Temperatura en kelvin a partir del texto del proyecto o de la CLI.
fn parse_temperature(text: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let quantity = parse_quantity(text)?;
    if quantity.dim() != Dimension::TEMPERATURE {
        return Err(format!("la temperatura debe ser una temperatura: {text}").into());
    }
    Ok(quantity.si)
}

/// Densidad que se asume si el proyecto no guarda `rho`: la del agua a su
/// temperatura (20 °C si no la indica), o 998,2 kg/m³ si no es agua o la
/// temperatura cae fuera de la correlación.
fn default_density(project: &Project) -> f64 {
    let temperature = if project.operating.temperature.trim().is_empty() {
        Some(293.15)
    } else {
        parse_temperature(&project.operating.temperature).ok()
    };
    let water =
        project.operating.fluid.is_empty() || project.operating.fluid.eq_ignore_ascii_case("water");
    temperature
        .filter(|_| water)
        .and_then(water_density)
        .unwrap_or(998.2)
}

/// Coherencia entre fluido, temperatura y densidad.
///
/// Solo se puede comprobar el agua: su densidad se calcula con Kell (1975)
/// entre 0 y 150 °C. Un texto de temperatura inválido lo señala la
/// validación del proyecto, no esta función.
fn fluid_diagnostics(fluid: &str, temperature: &str, rho_si: f64) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    if !fluid.eq_ignore_ascii_case("water") {
        return diagnostics;
    }
    let temperature_k = if temperature.trim().is_empty() {
        293.15
    } else {
        match parse_temperature(temperature) {
            Ok(value) => value,
            Err(_) => return diagnostics,
        }
    };
    let celsius = temperature_k - 273.15;

    match water_density(temperature_k) {
        None => diagnostics.push(
            Diagnostic::warning(
                "HM-DES-010",
                format!(
                    "el agua a {celsius:.1} °C queda fuera de la correlación de densidad (0..150 °C); \
                     no se comprobó rho"
                ),
            )
            .with_hint("Comprueba que rho corresponda a la temperatura de operación."),
        ),
        Some(expected) if ((rho_si - expected) / expected).abs() > WATER_DENSITY_TOLERANCE => {
            diagnostics.push(
                Diagnostic::warning(
                    "HM-DES-009",
                    format!(
                        "rho = {rho_si:.1} kg/m3 no es la densidad del agua a {celsius:.1} °C \
                         ({expected:.1} kg/m3)"
                    ),
                )
                .with_hint("Corrige rho o, si el fluido no es agua, usa fluid = \"custom\"."),
            );
        }
        Some(_) => {}
    }
    diagnostics
}

fn add_fluid_temperature_diagnostics(
    project: &Project,
    input: &DesignInput,
    diagnostics: &mut Diagnostics,
) {
    diagnostics.extend(
        fluid_diagnostics(
            &project.operating.fluid,
            &project.operating.temperature,
            input.rho.si,
        )
        .into_vec(),
    );
    diagnostics
        .extend(npsh_fluid_diagnostics(&project.operating.fluid, input.npsha.is_some()).into_vec());
}

/// El NPSHA necesario de `hm-design` usa el factor de fluido del agua
/// (FF = 1, Gülich 2020, tabla 6.2). Con otro fluido se avisa.
fn npsh_fluid_diagnostics(fluid: &str, npsh_checked: bool) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    if npsh_checked && !fluid.eq_ignore_ascii_case("water") {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-018",
                format!(
                    "el NPSHA necesario supone agua (FF = 1) y el fluido es `{fluid}`; \
                     la tabla 6.2 de Gülich pide FF ≥ 1,15 con agua de mar o fluidos corrosivos \
                     y otros criterios con hidrocarburos"
                ),
            )
            .with_hint("Aumenta el NPSHA en consecuencia o confirma el NPSHR con el fabricante."),
        );
    }
    diagnostics
}

fn design_audit(project: &Project) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    if LEGACY_METHODS.contains(&project.design.method.as_str()) {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-025",
                format!(
                    "el proyecto se diseñó con {}; el motor actual es {METHOD} y puede dar otros resultados",
                    project.design.method
                ),
            )
            .with_hint(
                "Ejecuta `hmflow-cli recalc --reselect <archivo>` para migrarlo con las \
                 selecciones nuevas, o `recalc` sin --reselect para conservar las tuyas.",
            ),
        );
        return diagnostics;
    }

    if !project.design.method.is_empty() && project.design.method != METHOD {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-016",
                format!("método de diseño desconocido: {}", project.design.method),
            )
            .with_hint(format!("Métodos conocidos: {METHOD}.")),
        );
        return diagnostics;
    }

    if project.design.method == METHOD && project.design.pump_type != PumpType::EndSuction {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-016",
                format!(
                    "el método {METHOD} no aplica a {}",
                    project.design.pump_type
                ),
            )
            .with_hint("Corrige el tipo de bomba o el método de diseño."),
        );
        return diagnostics;
    }

    if project.design.pump_type == PumpType::EndSuction
        && (project.design.method == METHOD || !project.params.is_empty())
    {
        validate_design_schema(project, &mut diagnostics);
    }

    match recompute_design_diagnostics(project) {
        Ok(Some(design_diagnostics)) => {
            diagnostics.extend(design_diagnostics.into_vec());
        }
        Ok(None) => diagnostics.push(Diagnostic::info(
            "HM-PROJ-021",
            format!(
                "{} todavía no tiene motor de diseño; solo se validó el formato del proyecto",
                project.design.pump_type
            ),
        )),
        Err(error) => {
            diagnostics.push(
                Diagnostic::error(
                    "HM-PROJ-014",
                    format!("no se pudo recalcular el diseño: {error}"),
                )
                .with_hint("Revisa la densidad, la eficiencia mecánica y el punto de operación."),
            );
        }
    }

    diagnostics
}

fn validate_design_schema(project: &Project, diagnostics: &mut Diagnostics) {
    for name in project.params.keys() {
        if param_spec(name).is_none() {
            diagnostics.push(
                Diagnostic::error("HM-PROJ-023", format!("parámetro desconocido: `{name}`"))
                    .with_hint("Revisa el nombre o vuelve a generar el diseño."),
            );
        }
    }

    for spec in PARAM_SPECS {
        let Some(param) = project.params.get(spec.name) else {
            diagnostics.push(
                Diagnostic::error("HM-PROJ-017", format!("falta el parámetro `{}`", spec.name))
                    .with_hint("Vuelve a ejecutar el diseño."),
            );
            continue;
        };

        match (spec.formula, param) {
            (Some(expected), Param::Auto { formula, value }) => {
                if formula != expected {
                    diagnostics.push(Diagnostic::error(
                        "HM-PROJ-018",
                        format!("`{}` usa `{formula}`; se esperaba `{expected}`", spec.name),
                    ));
                }
                match value {
                    Some(value) => check_parameter_unit(diagnostics, spec, value),
                    None => diagnostics.push(Diagnostic::error(
                        "HM-PROJ-017",
                        format!("`{}` no guarda su valor calculado", spec.name),
                    )),
                }
            }
            (Some(_), _) => diagnostics.push(
                Diagnostic::error("HM-PROJ-018", format!("`{}` debe ser auto", spec.name))
                    .with_hint(
                        "Ejecuta `hmflow-cli recalc <archivo>`: lo convierte y calcula su valor.",
                    ),
            ),
            (None, Param::Manual { value } | Param::Locked { value }) => {
                check_parameter_unit(diagnostics, spec, value);
                if spec.name == "blade_count" {
                    check_blade_count(diagnostics, value);
                }
            }
            (None, Param::Override { .. }) => diagnostics.push(
                Diagnostic::error(
                    "HM-PROJ-019",
                    format!(
                        "`{}` está en override; la auditoría no evalúa overrides",
                        spec.name
                    ),
                )
                .with_hint("Usa manual o locked."),
            ),
            (None, Param::Auto { .. }) => diagnostics.push(Diagnostic::error(
                "HM-PROJ-018",
                format!("`{}` debe ser manual o locked", spec.name),
            )),
        }
    }
}

fn check_parameter_unit(diagnostics: &mut Diagnostics, spec: &hm_design::ParamSpec, text: &str) {
    let expected = match parse_unit(spec.unit) {
        Ok(unit) => unit,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                "HM-PROJ-020",
                format!("unidad esperada inválida para `{}`: {error}", spec.name),
            ));
            return;
        }
    };

    match parse_quantity(text) {
        Ok(quantity) => {
            if !quantity.si.is_finite() {
                diagnostics.push(Diagnostic::error(
                    "HM-PROJ-020",
                    format!("`{}` debe ser finito: {text}", spec.name),
                ));
            } else if quantity.dim() != expected.dim {
                diagnostics.push(Diagnostic::error(
                    "HM-PROJ-020",
                    format!("`{}` debe ser {}: {text}", spec.name, spec.unit),
                ));
            }
        }
        Err(error) => diagnostics.push(Diagnostic::error(
            "HM-PROJ-020",
            format!("`{}` no se puede interpretar: {text} ({error})", spec.name),
        )),
    }
}

fn check_blade_count(diagnostics: &mut Diagnostics, text: &str) {
    if let Ok(quantity) = parse_quantity(text)
        && (quantity.si.fract() != 0.0 || !(1.0..=100.0).contains(&quantity.si))
    {
        diagnostics.push(Diagnostic::error(
            "HM-PROJ-020",
            format!("`blade_count` debe ser un entero entre 1 y 100: {text}"),
        ));
    }
}

fn compare_stored_autos(result: &DesignResult, project: &Project, diagnostics: &mut Diagnostics) {
    for (name, param) in &project.params {
        let Param::Auto {
            value: Some(text), ..
        } = param
        else {
            continue;
        };
        let Some(expected) = designed_si(result, name) else {
            continue;
        };

        match parse_quantity(text) {
            Ok(actual) if stored_si_matches(expected, actual.si) => {}
            Ok(actual) => {
                let shown = Quantity::from_si(expected, actual.unit);
                diagnostics.push(
                    Diagnostic::error(
                        "HM-PROJ-015",
                        format!(
                            "`{name}` guardado como «{text}» no coincide con el diseño recalculado ({:.6}{})",
                            shown.value(),
                            unit_suffix(&shown.unit.symbol)
                        ),
                    )
                    .with_hint(
                        "Ejecuta `hmflow-cli recalc <archivo>` para regenerar los parámetros automáticos.",
                    ),
                );
            }
            Err(error) => {
                diagnostics.push(
                    Diagnostic::error(
                        "HM-PROJ-015",
                        format!(
                            "`{name}` guardado como «{text}» no se puede comparar con el diseño ({error})"
                        ),
                    )
                    .with_hint(
                        "Ejecuta `hmflow-cli recalc <archivo>` para regenerar los parámetros automáticos.",
                    ),
                );
            }
        }
    }
}

/// Unidad para mostrar detrás de un número; vacía si es adimensional.
fn unit_suffix(symbol: &str) -> String {
    if symbol == "1" {
        String::new()
    } else {
        format!(" {symbol}")
    }
}

fn param_value(param: &Param) -> Option<&str> {
    match param {
        Param::Manual { value } | Param::Locked { value } => Some(value),
        Param::Auto { value, .. } => value.as_deref(),
        Param::Override { .. } => None,
    }
}

fn manual(value: String) -> Param {
    Param::Manual { value }
}

fn auto(formula: &str, value: String) -> Param {
    Param::Auto {
        formula: formula.to_string(),
        value: Some(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(diagnostics: &Diagnostics) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }

    #[test]
    fn fresh_end_suction_clears_the_design_check() {
        let diagnostics = design_audit(&Project::new("demo", PumpType::EndSuction));
        assert!(
            !diagnostics.has_errors(),
            "códigos: {:?}",
            codes(&diagnostics)
        );
    }

    #[test]
    fn other_pump_types_skip_the_end_suction_recompute() {
        let diagnostics = design_audit(&Project::new("demo", PumpType::Barrel));
        assert_eq!(codes(&diagnostics), ["HM-PROJ-021"]);
        assert!(!diagnostics.has_errors());
    }

    fn sample_design() -> DesignResult {
        design(&DesignInput {
            q: parse_quantity("120 m3/h").unwrap(),
            h: parse_quantity("45 m").unwrap(),
            n: parse_quantity("2900 rpm").unwrap(),
            rho: parse_quantity("998.2 kg/m3").unwrap(),
            eta_m: 0.96,
            npsha: Some(parse_quantity("10 m").unwrap()),
            nu: None,
        })
        .unwrap()
    }

    #[test]
    fn end_suction_method_on_barrel_is_reported() {
        let mut project = Project::new("demo", PumpType::Barrel);
        project.design.method = METHOD.to_string();

        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-016"));
    }

    #[test]
    fn unknown_method_is_reported() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = "end_suction_v9".to_string();

        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-016"));
    }

    #[test]
    fn unknown_only_parameter_is_reported() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project
            .params
            .insert("d22".to_string(), manual("0.2 m".to_string()));

        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-023"));
    }

    #[test]
    fn unknown_parameter_is_reported() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = METHOD.to_string();
        project
            .params
            .insert("d22".to_string(), manual("0.2 m".to_string()));

        let mut diagnostics = Diagnostics::new();
        validate_design_schema(&project, &mut diagnostics);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-023"));
    }

    #[test]
    fn missing_parameter_is_reported() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = METHOD.to_string();

        let mut diagnostics = Diagnostics::new();
        validate_design_schema(&project, &mut diagnostics);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-017"));
    }

    #[test]
    fn wrong_formula_is_reported() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = METHOD.to_string();
        project
            .params
            .insert("d2".to_string(), auto("design.d1", "0.2 m".to_string()));

        let mut diagnostics = Diagnostics::new();
        validate_design_schema(&project, &mut diagnostics);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-018"));
    }

    #[test]
    fn override_is_not_audited() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = METHOD.to_string();
        project.params.insert(
            "psi".to_string(),
            Param::Override {
                expr: "0.8".to_string(),
                author: "test".to_string(),
                date: "2026-11-01".to_string(),
                reason: "prueba".to_string(),
            },
        );

        let mut diagnostics = Diagnostics::new();
        validate_design_schema(&project, &mut diagnostics);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-019"));
    }

    #[test]
    fn blade_count_must_be_an_integer() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = METHOD.to_string();
        project
            .params
            .insert("blade_count".to_string(), manual("7.5".to_string()));

        let mut diagnostics = Diagnostics::new();
        validate_design_schema(&project, &mut diagnostics);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-020"));
    }

    #[test]
    fn manual_psi_changes_the_recomputed_design() {
        let designed = sample_design();
        let mut project = Project::new("demo", PumpType::EndSuction);
        project
            .params
            .insert("psi".to_string(), manual("0.5".to_string()));
        project.params.insert(
            "d2".to_string(),
            auto("design.d2", format!("{} m", designed.d2.value())),
        );

        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-015"));
    }

    #[test]
    fn water_with_high_density_warns() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project
            .params
            .insert("rho".to_string(), manual("1200 kg/m3".to_string()));

        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-DES-009"));
    }

    #[test]
    fn hot_water_with_cold_density_warns() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.operating.temperature = "80 degC".to_string();
        project
            .params
            .insert("rho".to_string(), manual("998.2 kg/m3".to_string()));

        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-DES-009"));
    }

    #[test]
    fn hot_water_with_its_density_is_quiet() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.operating.temperature = "80 degC".to_string();
        project
            .params
            .insert("rho".to_string(), manual("971.8 kg/m3".to_string()));

        let diagnostics = design_audit(&project);
        assert!(!codes(&diagnostics).contains(&"HM-DES-009"));
        assert!(!codes(&diagnostics).contains(&"HM-DES-010"));
    }

    #[test]
    fn missing_density_follows_the_water_temperature() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.operating.temperature = "80 degC".to_string();
        assert!((default_density(&project) - 971.8).abs() < 0.01);

        project.operating.fluid = "custom".to_string();
        assert!((default_density(&project) - 998.2).abs() < 1e-9);
    }

    #[test]
    fn water_outside_the_correlation_warns() {
        let diagnostics = fluid_diagnostics("water", "200 degC", 865.0);
        assert_eq!(codes(&diagnostics), ["HM-DES-010"]);
    }

    #[test]
    fn other_fluids_are_not_checked() {
        assert!(fluid_diagnostics("custom", "20 degC", 1200.0).is_empty());
        assert!(fluid_diagnostics("glycol", "80 degC", 1050.0).is_empty());
    }

    #[test]
    fn old_manual_slip_points_to_recalc() {
        let mut project = designed_project();
        project
            .params
            .insert("slip".to_string(), manual("0.84".to_string()));
        let mut diagnostics = Diagnostics::new();
        validate_design_schema(&project, &mut diagnostics);
        let hint = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "HM-PROJ-018")
            .and_then(|diagnostic| diagnostic.hint.clone())
            .unwrap_or_default();
        assert!(hint.contains("recalc"), "{hint}");
    }

    #[test]
    fn negative_expressions_and_values_parse() {
        let cli = Cli::try_parse_from(["hmflow-cli", "eval", "-2^2"]).unwrap();
        assert!(matches!(cli.command, Command::Eval { expr, .. } if expr == "-2^2"));

        let cli = Cli::try_parse_from(["hmflow-cli", "eval", "-a", "--var", "a=-3 m"]).unwrap();
        assert!(matches!(cli.command, Command::Eval { vars, .. } if vars == ["a=-3 m"]));

        let cli = Cli::try_parse_from(["hmflow-cli", "design", "--npsha", "-3 m"]).unwrap();
        assert!(matches!(cli.command, Command::Design { npsha: Some(text), .. } if text == "-3 m"));
    }

    #[test]
    fn npsh_criterion_warns_for_fluids_other_than_water() {
        assert_eq!(
            codes(&npsh_fluid_diagnostics("custom", true)),
            ["HM-DES-018"]
        );
        assert!(npsh_fluid_diagnostics("water", true).is_empty());
        assert!(npsh_fluid_diagnostics("custom", false).is_empty());
    }

    #[test]
    fn legacy_method_gets_a_single_migration_error() {
        let mut project = designed_project();
        project.design.method = "end_suction_v0.1".to_string();
        let diagnostics = design_audit(&project);
        assert_eq!(codes(&diagnostics), ["HM-PROJ-025"]);
    }

    #[test]
    fn recalc_migrates_a_v08_project() {
        // El .hmf de referencia de la v0.8, con A y J manuales. `recalc`
        // lo lleva a la v0.9 y conserva las selecciones manuales.
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hm-project/tests/fixtures/end_suction_v0.8.hmf");
        let path = std::env::temp_dir().join(format!(
            "hmflow-v08-{}-{:?}.hmf",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::copy(&fixture, &path).unwrap();
        let before = Project::load(&path).unwrap();
        assert_eq!(before.design.method, "end_suction_v0.8");

        recalc(&path, false).unwrap();
        let project = Project::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(project.design.method, "end_suction_v0.9");
        assert_eq!(
            format!("{:?}", project.params["J"]),
            format!("{:?}", before.params["J"])
        );
        let diagnostics = design_audit(&project);
        assert!(
            !diagnostics.has_errors(),
            "códigos: {:?}",
            codes(&diagnostics)
        );
    }

    #[test]
    fn recalc_migrates_a_legacy_project() {
        let path = std::env::temp_dir().join(format!(
            "hmflow-legacy-{}-{:?}.hmf",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut project = designed_project();
        project.design.method = "end_suction_v0.3".to_string();
        // Hasta la v0.3, ηm incluía el rozamiento de discos.
        project
            .params
            .insert("eta_m".to_string(), manual("0.96".to_string()));
        project.save(&path).unwrap();

        recalc(&path, true).unwrap();
        let project = Project::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(project.design.method, METHOD);
        let (input, _) = design_inputs(&project).unwrap();
        let expected = default_mechanical_efficiency(&input.q, &input.n).unwrap();
        assert!(
            (input.eta_m - expected).abs() < 1e-12,
            "eta_m {} frente a la estimación {expected}",
            input.eta_m
        );
        let diagnostics = design_audit(&project);
        assert!(
            !diagnostics.has_errors(),
            "códigos: {:?}",
            codes(&diagnostics)
        );
    }

    #[test]
    fn recalc_from_v04_keeps_the_mechanical_efficiency() {
        // En la v0.4 ηm ya no incluía los discos: migrar a la v0.5 no lo toca.
        let path = std::env::temp_dir().join(format!(
            "hmflow-v04-{}-{:?}.hmf",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut project = designed_project();
        project.design.method = "end_suction_v0.4".to_string();
        project
            .params
            .insert("eta_m".to_string(), manual("0.975".to_string()));
        project.save(&path).unwrap();

        recalc(&path, false).unwrap();
        let project = Project::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(project.design.method, METHOD);
        let (input, _) = design_inputs(&project).unwrap();
        assert!((input.eta_m - 0.975).abs() < 1e-12, "eta_m {}", input.eta_m);
    }

    #[test]
    fn report_of_a_designed_project_matches_its_stored_values() {
        let path = std::env::temp_dir().join(format!(
            "hmflow-report-{}-{:?}.hmf",
            std::process::id(),
            std::thread::current().id()
        ));
        let project = designed_project();
        project.save(&path).unwrap();
        let report = design_report(&path, 11);
        std::fs::remove_file(&path).unwrap();
        let report = report.unwrap();

        assert_eq!(report.method, METHOD);
        assert!(report.diagnostics.iter().all(|d| !d.is_error()));
        let geometry = report
            .geometry
            .as_ref()
            .expect("el informe lleva geometría");
        assert_eq!(
            geometry.impeller.blade.len(),
            hm_design::geometry::BLADE_STEPS + 1
        );
        assert!(geometry.volute_outlet.nominal_diameter.value > 0.0);
        let mechanics = report
            .mechanics
            .as_ref()
            .expect("el informe lleva mecánica");
        assert_eq!(
            mechanics.shaft_shear_stress.value,
            DEFAULT_SHAFT_SHEAR_STRESS
        );
        assert!(mechanics.shaft_min_diameter.value > 0.0);
        let stored = parse_quantity(param_value(&project.params["d2"]).unwrap())
            .unwrap()
            .si;
        assert!((report.results["d2"].value - stored).abs() < 1e-12);
    }

    /// Guarda de versionado: el `.hmf` de referencia se escribió con
    /// `end_suction_v0.9`. Si el motor cambia sus resultados sin cambiar
    /// `METHOD`, sus valores `auto` dejan de cuadrar y este test falla. La
    /// salida correcta es subir `METHOD`, añadir el anterior a
    /// `LEGACY_METHODS` y regenerar el archivo con `design`.
    #[test]
    fn reference_project_of_the_current_method_audits_clean() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hm-project/tests/fixtures/end_suction_v0.9.hmf");
        let project = Project::load(&path).unwrap();
        assert_eq!(
            project.design.method, METHOD,
            "METHOD cambió: regenera el .hmf de referencia y añade el método anterior a LEGACY_METHODS"
        );
        let diagnostics = design_audit(&project);
        assert!(
            !diagnostics.has_errors(),
            "códigos: {:?}",
            codes(&diagnostics)
        );
    }

    #[test]
    fn suction_npsha_uses_iapws_vapor_pressure_for_water() {
        let suction = Suction {
            pressure: "1 atm".to_string(),
            height: "-3 m".to_string(),
            losses: "0.5 m".to_string(),
            vapor_pressure: String::new(),
        };
        // (101325 − 2339,2)/(998,2·9,80665) − 3 − 0,5
        let npsha = suction_npsha(&suction, "water", "20 degC", 998.2).unwrap();
        assert!((npsha.si - 6.612).abs() < 1e-3, "{}", npsha.si);

        // Más caliente, más presión de vapor y menos NPSHa.
        let hot = suction_npsha(&suction, "water", "80 degC", 971.8).unwrap();
        assert!(hot.si < npsha.si - 4.0, "{}", hot.si);

        assert!(suction_npsha(&suction, "glycol", "20 degC", 1050.0).is_err());
        let mut given = suction.clone();
        given.vapor_pressure = "0 Pa".to_string();
        let no_vapor = suction_npsha(&given, "glycol", "20 degC", 1000.0).unwrap();
        assert!((no_vapor.si - (101_325.0 / (1000.0 * 9.80665) - 3.5)).abs() < 1e-9);
    }

    #[test]
    fn viscosity_can_be_kinematic_or_dynamic() {
        let cst = kinematic_viscosity("30 cSt", "oil", "", 900.0)
            .unwrap()
            .unwrap();
        assert!((cst - 30e-6).abs() < 1e-15);
        let cp = kinematic_viscosity("27 cP", "oil", "", 900.0)
            .unwrap()
            .unwrap();
        assert!((cp - 30e-6).abs() < 1e-15);
        let water = kinematic_viscosity("", "water", "20 degC", 998.2)
            .unwrap()
            .unwrap();
        assert!((water - 1.0034e-6).abs() < 1e-9);
        assert_eq!(kinematic_viscosity("", "oil", "", 900.0).unwrap(), None);
        assert!(kinematic_viscosity("3 m", "oil", "", 900.0).is_err());
    }

    #[test]
    fn viscosity_effect_reports_missing_and_large_effects() {
        let result = sample_design();
        let input = DesignInput {
            q: parse_quantity("120 m3/h").unwrap(),
            h: parse_quantity("45 m").unwrap(),
            n: parse_quantity("2900 rpm").unwrap(),
            rho: parse_quantity("900 kg/m3").unwrap(),
            eta_m: 0.96,
            npsha: None,
            nu: None,
        };
        let (diagnostics, correction) =
            viscosity_effect("oil", "20 degC", "", &input, &result).unwrap();
        assert_eq!(codes(&diagnostics), ["HM-DES-022"]);
        assert!(correction.is_none());

        let (diagnostics, correction) =
            viscosity_effect("oil", "20 degC", "100 cSt", &input, &result).unwrap();
        assert_eq!(codes(&diagnostics), ["HM-DES-020"]);
        assert!(correction.unwrap().f_eta < 0.85);
    }

    #[test]
    fn project_with_suction_audits_and_reports() {
        let mut project = designed_project();
        project.operating.npsha = String::new();
        project.suction = Some(Suction {
            pressure: "1 atm".to_string(),
            height: "6 m".to_string(),
            losses: String::new(),
            vapor_pressure: String::new(),
        });
        project.operating.viscosity = "20 cSt".to_string();
        // El proyecto se dimensionó con agua. ν entra en discos y anillo, así
        // que los auto guardados ya no cuadran (HM-PROJ-015) hasta un recalc.
        // Lo que no puede pasar es que el anillo deje el diseño sin resultado.
        let diagnostics = design_audit(&project);
        assert!(
            !codes(&diagnostics).contains(&"HM-PROJ-014"),
            "códigos: {:?}",
            codes(&diagnostics)
        );
        assert!(codes(&diagnostics).contains(&"HM-PROJ-015"));

        let path = std::env::temp_dir().join(format!(
            "hmflow-suction-{}-{:?}.hmf",
            std::process::id(),
            std::thread::current().id()
        ));
        project.save(&path).unwrap();
        let report = design_report(&path, 5);
        std::fs::remove_file(&path).unwrap();
        let report = report.unwrap();
        let npsha = report.input["npsha"].value;
        assert!(
            (npsha - (101_325.0 - 2339.2) / (998.204 * 9.80665) - 6.0).abs() < 0.01,
            "{npsha}"
        );
        let viscous = report.viscous.expect("con viscosidad hay corrección");
        assert!((viscous.nu.value - 20e-6).abs() < 1e-15);
        assert!(viscous.f_h.value < 1.0);
    }

    #[test]
    fn npsha_and_suction_pressure_exclude_each_other() {
        let result = Cli::try_parse_from([
            "hmflow-cli",
            "design",
            "--npsha",
            "10 m",
            "--suction-pressure",
            "1 atm",
        ]);
        assert!(result.is_err());
        assert!(Cli::try_parse_from(["hmflow-cli", "design", "--suction-height", "2 m"]).is_err());
    }

    #[test]
    fn densities_of_liquid_water_are_recognised() {
        assert!(looks_like_water(998.2));
        assert!(looks_like_water(971.8));
        assert!(looks_like_water(917.0));
        assert!(!looks_like_water(1200.0));
        assert!(!looks_like_water(850.0));
    }

    #[test]
    fn dimensionless_mismatch_has_no_unit_suffix() {
        assert_eq!(unit_suffix("1"), "");
        assert_eq!(unit_suffix("m3/h"), " m3/h");
    }

    #[test]
    fn a_density_with_the_wrong_dimension_fails_the_check() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project
            .params
            .insert("rho".to_string(), manual("1200 kg".to_string()));
        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-014"));
        assert!(diagnostics.has_errors());
    }

    #[test]
    fn an_efficiency_outside_the_open_interval_fails_the_check() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project
            .params
            .insert("eta_m".to_string(), manual("2".to_string()));
        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-014"));
    }

    #[test]
    fn a_tampered_auto_value_fails_the_check() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project
            .params
            .insert("d2".to_string(), auto("design.d2", "9 m".to_string()));
        let diagnostics = design_audit(&project);
        assert!(
            codes(&diagnostics).contains(&"HM-PROJ-015"),
            "códigos: {:?}",
            codes(&diagnostics)
        );
    }

    #[test]
    fn a_rounded_auto_value_matches_the_recomputed_design() {
        let project = Project::new("demo", PumpType::EndSuction);
        let result = recompute_design_diagnostics(&project)
            .unwrap()
            .expect("end-suction recalcula");
        assert!(!result.has_errors());

        let designed = design(&DesignInput {
            q: parse_quantity("120 m3/h").unwrap(),
            h: parse_quantity("45 m").unwrap(),
            n: parse_quantity("2900 rpm").unwrap(),
            rho: parse_quantity("998.2 kg/m3").unwrap(),
            eta_m: 0.96,
            npsha: Some(parse_quantity("10 m").unwrap()),
            nu: None,
        })
        .unwrap();

        let mut project = project;
        project.params.insert(
            "d2".to_string(),
            auto("design.d2", format!("{} m", designed.d2.value())),
        );
        project.params.insert(
            "dn".to_string(),
            auto(
                "design.specific_diameter",
                format!("{:.4}", designed.specific_diameter_mm),
            ),
        );
        let diagnostics = design_audit(&project);
        assert!(
            !codes(&diagnostics).contains(&"HM-PROJ-015"),
            "códigos: {:?}",
            codes(&diagnostics)
        );
    }

    /// Proyecto completo tal como lo escribe `design` para la duty de ejemplo.
    fn designed_project() -> Project {
        let result = sample_design();
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = METHOD.to_string();
        project.operating.npsha = "15 m".to_string();
        let defaults = ManualDefaults {
            rho: "998.2 kg/m3".to_string(),
            eta_m: "0.96".to_string(),
        };
        project.params = design_params(&result, None, &defaults).unwrap().0;
        project
    }

    fn recalculated(project: &Project) -> Project {
        let (input, options) = design_inputs(project).unwrap();
        let result = design_with_options(&input, &options).unwrap();
        let defaults = ManualDefaults {
            rho: "998.2 kg/m3".to_string(),
            eta_m: "0.96".to_string(),
        };
        let mut project = project.clone();
        project.params = design_params(&result, Some(&project.params), &defaults)
            .unwrap()
            .0;
        project
    }

    #[test]
    fn a_designed_project_passes_the_check() {
        let diagnostics = design_audit(&designed_project());
        assert!(
            !diagnostics.has_errors(),
            "códigos: {:?}",
            codes(&diagnostics)
        );
    }

    #[test]
    fn slip_and_cu2_are_stored_as_auto() {
        let project = designed_project();
        for name in ["slip", "cu2"] {
            assert!(
                matches!(project.params.get(name), Some(Param::Auto { .. })),
                "`{name}` debe ser auto"
            );
        }
    }

    #[test]
    fn editing_beta2_or_blade_count_is_detected() {
        for (name, value) in [("beta2", "35 deg"), ("blade_count", "4")] {
            let mut project = designed_project();
            project
                .params
                .insert(name.to_string(), manual(value.to_string()));
            let diagnostics = design_audit(&project);
            assert!(
                codes(&diagnostics).contains(&"HM-PROJ-015"),
                "editar `{name}` no se detectó: {:?}",
                codes(&diagnostics)
            );
        }
    }

    #[test]
    fn recalc_keeps_manual_edits_and_clears_the_check() {
        let mut project = designed_project();
        project
            .params
            .insert("beta2".to_string(), manual("30 deg".to_string()));
        assert!(design_audit(&project).has_errors());

        let project = recalculated(&project);
        let diagnostics = design_audit(&project);
        assert!(
            !diagnostics.has_errors(),
            "códigos: {:?}",
            codes(&diagnostics)
        );
        assert_eq!(
            project.params.get("beta2").and_then(param_value),
            Some("30 deg")
        );
    }

    #[test]
    fn recalc_fills_missing_manual_parameters() {
        let mut project = designed_project();
        project.params.remove("k_eff");
        let (input, options) = design_inputs(&project).unwrap();
        let result = design_with_options(&input, &options).unwrap();
        let defaults = ManualDefaults {
            rho: "998.2 kg/m3".to_string(),
            eta_m: "0.96".to_string(),
        };
        let (params, filled) = design_params(&result, Some(&project.params), &defaults).unwrap();
        assert_eq!(filled, ["k_eff"]);
        assert!(params.contains_key("k_eff"));
    }

    #[test]
    fn recalc_does_not_replace_an_override() {
        let mut project = designed_project();
        project.params.insert(
            "mm".to_string(),
            Param::Override {
                expr: "0.001".to_string(),
                author: "test".to_string(),
                date: "2026-11-02".to_string(),
                reason: "prueba".to_string(),
            },
        );
        let result = sample_design();
        let defaults = ManualDefaults {
            rho: "998.2 kg/m3".to_string(),
            eta_m: "0.96".to_string(),
        };
        assert!(design_params(&result, Some(&project.params), &defaults).is_err());
    }

    #[test]
    fn selections_outside_their_domain_fail_the_check() {
        for (name, value) in [("margin", "0.5"), ("k_eff", "0"), ("k3", "1")] {
            let mut project = designed_project();
            project
                .params
                .insert(name.to_string(), manual(value.to_string()));
            let diagnostics = design_audit(&project);
            assert!(
                codes(&diagnostics).contains(&"HM-PROJ-014"),
                "`{name}` = {value} no se rechazó: {:?}",
                codes(&diagnostics)
            );
        }
    }

    /// Proyecto diseñado para 120 m3/h cuyo caudal pasa a 250 m3/h: nq sube
    /// de 30,5 a 44 y cambia de tramo en la tabla.
    fn moved_duty_project() -> Project {
        let mut project = designed_project();
        project.operating.q = "250 m3/h".to_string();
        project.operating.npsha = "25 m".to_string();
        project
    }

    #[test]
    fn selections_from_another_duty_are_flagged() {
        let diagnostics = design_audit(&moved_duty_project());
        assert!(
            codes(&diagnostics).contains(&"HM-DES-013"),
            "códigos: {:?}",
            codes(&diagnostics)
        );
        assert!(!codes(&designed_project_diagnostics()).contains(&"HM-DES-013"));
    }

    fn designed_project_diagnostics() -> Diagnostics {
        design_audit(&designed_project())
    }

    #[test]
    fn locked_selections_are_not_flagged() {
        let mut project = moved_duty_project();
        for name in NQ_SELECTIONS {
            let value = param_value(&project.params[name]).unwrap().to_string();
            project
                .params
                .insert(name.to_string(), Param::Locked { value });
        }
        assert_eq!(stale_selections(&project), Vec::<&str>::new());
    }

    #[test]
    fn reselect_takes_the_table_for_the_new_nq() {
        let path = std::env::temp_dir().join(format!(
            "hmflow-reselect-{}-{:?}.hmf",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut project = moved_duty_project();
        project.params.insert(
            "blade_count".to_string(),
            Param::Locked {
                value: "7".to_string(),
            },
        );
        project.save(&path).unwrap();

        recalc(&path, true).unwrap();
        let project = Project::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        // ψ de Gülich (3.26) para nq = 44.
        let psi: f64 = project
            .params
            .get("psi")
            .and_then(param_value)
            .unwrap()
            .parse()
            .unwrap();
        assert!(
            (psi - 1.21 * (-0.77f64 * 0.4398).exp()).abs() < 1e-3,
            "ψ = {psi}"
        );
        assert_eq!(
            project.params.get("blade_count").and_then(param_value),
            Some("7"),
            "una selección locked no se toca"
        );
        let diagnostics = design_audit(&project);
        assert!(
            !diagnostics.has_errors(),
            "códigos: {:?}",
            codes(&diagnostics)
        );
        assert!(!codes(&diagnostics).contains(&"HM-DES-013"));
    }

    #[test]
    fn recalc_without_reselect_keeps_the_old_selections() {
        let path = std::env::temp_dir().join(format!(
            "hmflow-keep-{}-{:?}.hmf",
            std::process::id(),
            std::thread::current().id()
        ));
        let before = moved_duty_project();
        let psi_before = before
            .params
            .get("psi")
            .and_then(param_value)
            .unwrap()
            .to_string();
        before.save(&path).unwrap();

        recalc(&path, false).unwrap();
        let project = Project::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(
            project.params.get("psi").and_then(param_value),
            Some(psi_before.as_str())
        );
        assert!(codes(&design_audit(&project)).contains(&"HM-DES-013"));
    }
}
