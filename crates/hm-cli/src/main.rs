//! CLI de HidroMac Flow.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hm_core::Diagnostics;
use hm_design::geometry::geometry;
use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics, mechanics_diagnostics};
use hm_design::{
    DEFAULT_K_EFF, DEFAULT_VOLUTE_C, DesignInput, DesignOptions, DesignReport, DesignResult,
    LEGACY_METHODS, METHOD, PARAM_SPECS, ParamSpec, default_mechanical_efficiency,
    design_with_options, designed_si, param_spec, performance_curve,
};
use hm_engine::{
    Density, DensitySource, Entered, Fluid, Intake, Temperature, add_fluid_temperature_diagnostics,
    add_selection_diagnostics, audit, cli_message, fluid_diagnostics, fluid_kind,
    kinematic_viscosity, kinematic_viscosity_of, looks_like_water, npsh_fluid_diagnostics,
    parse_temperature, project_viscosity_effect, suction_npsha, viscosity_effect,
    viscosity_from_text, water_density_at,
};
use hm_formula::{Registry, eval, infer, parse};
use hm_project::{Param, Project, PumpType, Suction};
use hm_units::{Quantity, parse_quantity, parse_unit};

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
        /// Rendimiento volumétrico del rodete, entre 0 y 1. Sin él, el de
        /// Gülich 3.5.10.
        #[arg(long)]
        eta_v: Option<f64>,
        /// El rodete tiene agujeros de equilibrio (la fuga se duplica). Con
        /// `--eta-v` no tiene efecto: el ηv fijado manda.
        #[arg(long)]
        balance_holes: bool,
        /// Razón c3q/c2 de la garganta de la voluta, entre 0,5 y 1. Por
        /// defecto 0,8 (Gülich, tabla 10.2); con 1 sale la de Srinivasan.
        #[arg(long)]
        volute_ratio: Option<f64>,
        /// Rugosidad de arena equivalente del rodete, por ejemplo "0.05 mm" (50 µm).
        /// Sin ella el rodete se trata como liso.
        #[arg(long, allow_hyphen_values = true)]
        roughness: Option<String>,
        /// Lenguas de la voluta: 1 simple (por defecto), 2 doble. Con un
        /// número par solo se eligen álabes impares.
        #[arg(long)]
        volute_channels: Option<u32>,
        /// Ángulo de envolvente de la voluta doble, en grados (90..180). Sin
        /// él, voluta simple.
        #[arg(long, allow_hyphen_values = true)]
        volute_wrap: Option<f64>,
        /// Incidencia del álabe a la entrada, en grados (0..10; 0..4 según
        /// Gülich). Sin ella, 2°.
        #[arg(long, allow_hyphen_values = true)]
        incidence: Option<f64>,
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
            let diagnostics = audit(&project);
            if diagnostics.is_empty() {
                println!("{}: sin diagnósticos", file.display());
            } else {
                print_diagnostics(&diagnostics);
            }

            if diagnostics.has_errors() {
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
            eta_v,
            balance_holes,
            volute_ratio,
            roughness,
            volute_channels,
            volute_wrap,
            incidence,
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
            let temperature_value = parse_temperature(&temperature).map_err(engine_text)?;
            let rho_text = match rho {
                Some(text) => text,
                None => {
                    if fluid
                        .as_deref()
                        .is_some_and(|fluid| !fluid.eq_ignore_ascii_case("water"))
                    {
                        return Err("indica --rho: solo se conoce la densidad del agua".into());
                    }
                    let density = water_density_at(&temperature_value).ok_or(
                        "la temperatura está fuera de 0..150 °C, el intervalo de la densidad del agua; indica --rho",
                    )?;
                    format!("{:.3} kg/m3", density.si)
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
            let intake = match &suction {
                Some(suction) => Intake::Installation {
                    pressure: Entered::parse(&suction.pressure)?,
                    height: optional_entered(&suction.height)?,
                    losses: optional_entered(&suction.losses)?,
                    vapor_pressure: optional_entered(&suction.vapor_pressure)?,
                },
                None => match npsha.as_deref() {
                    Some(text) => Intake::Available(Entered::parse(text)?),
                    None => Intake::None,
                },
            };
            let mut case_fluid = Fluid {
                kind: fluid_kind(&fluid),
                temperature: Some(temperature_value),
                density: Density {
                    value: rho_quantity.clone(),
                    source: DensitySource::Explicit {
                        text: rho_text.clone(),
                    },
                },
                viscosity: None,
            };
            let npsha_quantity = hm_engine::npsha(&intake, &case_fluid).map_err(engine_text)?;
            let q_quantity = parse_quantity(&q)?;
            let n_quantity = parse_quantity(&n)?;
            let eta_m = match eta_m {
                Some(eta_m) => eta_m,
                None => default_mechanical_efficiency(&q_quantity, &n_quantity)?,
            };
            let viscosity_text = viscosity.unwrap_or_default();
            case_fluid.viscosity = viscosity_from_text(&viscosity_text).map_err(engine_text)?;
            let nu = kinematic_viscosity(&case_fluid).map_err(engine_text)?;
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

            let options = DesignOptions {
                eta_v,
                balance_holes,
                volute_ratio,
                impeller_roughness: roughness
                    .as_deref()
                    .map(|text| -> Result<f64, Box<dyn std::error::Error>> {
                        let quantity = parse_quantity(text)?;
                        if quantity.dim() != parse_unit("m")?.dim {
                            return Err(
                                "--roughness debe ser una longitud, por ejemplo \"0.05 mm\"".into(),
                            );
                        }
                        Ok(quantity.si)
                    })
                    .transpose()?,
                volute_channels,
                volute_wrap_deg: volute_wrap,
                incidence_deg: incidence,
                ..DesignOptions::default()
            };
            let result = design_with_options(&input, &options)?;

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
                "  Pérdidas Gülich: discos PRR/Pu {:.3}   ηh estadístico {:.3} (dimensionado {:.3})   ηv {:.3}   ηm {:.3}   → η {:.3}",
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
                "  Empuje radial ({}): {:.0} N cerrada   {:.0} N diseño   {:.0} N a {:.1}·Qd   axial ≈ {:.0}..{:.0} N",
                match result.volute.wrap_deg {
                    Some(wrap) => format!("voluta doble, ε = {wrap:.0}°"),
                    None => "voluta simple".to_string(),
                },
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
            shown.extend(fluid_diagnostics(&case_fluid).into_vec());
            shown.extend(npsh_fluid_diagnostics(&case_fluid, input.npsha.is_some()).into_vec());
            let (viscous_diagnostics, correction) =
                viscosity_effect(&case_fluid, &input, &result).map_err(engine_text)?;
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

            let defaults = ManualDefaults::with_options(rho_text, eta_m.to_string(), &options);
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
        Some(suction) => Some(
            suction_npsha(
                suction,
                &project.operating.fluid,
                &project.operating.temperature,
                rho.si,
            )
            .map_err(engine_text)?,
        ),
        None if project.operating.npsha.trim().is_empty() => None,
        None => Some(parse_quantity(&project.operating.npsha)?),
    };
    let options = design_options(project)?;
    let nu = kinematic_viscosity_of(
        &project.operating.viscosity,
        &project.operating.fluid,
        &project.operating.temperature,
        rho.si,
    )
    .map_err(engine_text)?;

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

/// Selecciones que `recalc --reselect` vuelve a tomar del motor para el punto.
///
/// J entra porque el ojo automático lo fija junto con c0 y cm0. Si se deja
/// el J de otro punto, el motor no minimiza el NPSH y c0 queda distinto.
/// La auditoría del motor repite esta lista para detectar las que quedaron viejas.
const NQ_SELECTIONS: [&str; 7] = ["psi", "phi", "beta2", "blade_count", "c0", "cm0", "J"];

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
        project_viscosity_effect(&project, &input, &result)
            .map_err(engine_text)?
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

    let defaults =
        ManualDefaults::engine(format!("{} kg/m3", input.rho.si), input.eta_m.to_string());
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
    diagnostics.extend(audit(&project).into_vec());
    let report = DesignReport::new(&input, &result, curve_points, diagnostics)
        .with_geometry(&geometry(&input, &result)?)
        .with_mechanics(&mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS)?);
    Ok(
        match project_viscosity_effect(&project, &input, &result)
            .map_err(engine_text)?
            .1
        {
            Some(correction) => report.with_viscous(&correction),
            None => report,
        },
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
    /// Opciones del método v0.9 (tarea 12), como texto de su parámetro.
    eta_v_fixed: String,
    balance_holes: String,
    volute_ratio: String,
    impeller_roughness: String,
    volute_channels: String,
    volute_wrap_deg: String,
    incidence_deg: String,
}

impl ManualDefaults {
    /// Con las opciones por defecto del motor.
    fn engine(rho: String, eta_m: String) -> Self {
        Self::with_options(rho, eta_m, &DesignOptions::default())
    }

    /// Con los valores de `options`; «0» donde no hay dato.
    fn with_options(rho: String, eta_m: String, options: &DesignOptions) -> Self {
        Self {
            rho,
            eta_m,
            eta_v_fixed: options.eta_v.unwrap_or(0.0).to_string(),
            balance_holes: u8::from(options.balance_holes).to_string(),
            volute_ratio: options
                .volute_ratio
                .unwrap_or(hm_design::DEFAULT_VOLUTE_RATIO)
                .to_string(),
            impeller_roughness: format!("{} m", options.impeller_roughness.unwrap_or(0.0)),
            volute_channels: options.volute_channels.unwrap_or(1).to_string(),
            volute_wrap_deg: format!("{} deg", options.volute_wrap_deg.unwrap_or(0.0)),
            incidence_deg: format!(
                "{} deg",
                options
                    .incidence_deg
                    .unwrap_or(hm_design::geometry::DEFAULT_INCIDENCE_DEG)
            ),
        }
    }
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
        "eta_v_fixed" => defaults.eta_v_fixed.clone(),
        "balance_holes" => defaults.balance_holes.clone(),
        "volute_ratio" => defaults.volute_ratio.clone(),
        "impeller_roughness" => defaults.impeller_roughness.clone(),
        "volute_channels" => defaults.volute_channels.clone(),
        "volute_wrap_deg" => defaults.volute_wrap_deg.clone(),
        "incidence_deg" => defaults.incidence_deg.clone(),
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
        // «0» significa sin dato: ηv estimado, rodete liso, voluta simple.
        eta_v: param_f64(project, "eta_v_fixed", "1")?.filter(|value| *value != 0.0),
        balance_holes: match param_f64(project, "balance_holes", "1")? {
            None => false,
            Some(value) if value == 0.0 || value == 1.0 => value == 1.0,
            Some(_) => return Err("`balance_holes` debe ser 0 o 1".into()),
        },
        volute_ratio: param_f64(project, "volute_ratio", "1")?.filter(|value| *value != 0.0),
        impeller_roughness: param_f64(project, "impeller_roughness", "m")?
            .filter(|value| *value != 0.0),
        // «0» = sin dato, como en las demás opciones numéricas.
        volute_channels: if param_f64(project, "volute_channels", "1")? == Some(0.0) {
            None
        } else {
            param_u32(project, "volute_channels")?
        },
        volute_wrap_deg: param_f64(project, "volute_wrap_deg", "deg")?
            .filter(|value| *value != 0.0)
            .map(f64::to_degrees),
        incidence_deg: param_f64(project, "incidence_deg", "deg")?.map(f64::to_degrees),
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

/// Densidad que se asume si el proyecto no guarda `rho`: la del agua a su
/// temperatura (20 °C si no la indica), o 998,2 kg/m³ si no es agua o la
/// temperatura cae fuera de la correlación.
///
/// Un texto de temperatura ilegible no aborta: se usa la densidad por defecto,
/// igual que antes de pasar el cálculo al motor.
fn default_density(project: &Project) -> f64 {
    let water =
        project.operating.fluid.is_empty() || project.operating.fluid.eq_ignore_ascii_case("water");
    if !water {
        return 998.2;
    }
    let temperature = if project.operating.temperature.trim().is_empty() {
        Temperature {
            text: String::new(),
            kelvin: 293.15,
        }
    } else {
        match parse_temperature(&project.operating.temperature) {
            Ok(temperature) => temperature,
            Err(_) => return 998.2,
        }
    };
    water_density_at(&temperature)
        .map(|density| density.si)
        .unwrap_or(998.2)
}

fn optional_entered(text: &str) -> Result<Option<Entered>, hm_units::UnitError> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(Entered::parse(text)?))
    }
}

/// La CLI sigue mostrando la frase de antes. El código queda en el motor.
fn engine_text(error: hm_engine::EngineError) -> Box<dyn std::error::Error> {
    cli_message(&error).into()
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
    use hm_design::design;

    fn codes(diagnostics: &Diagnostics) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
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
    fn negative_expressions_and_values_parse() {
        let cli = Cli::try_parse_from(["hmflow-cli", "eval", "-2^2"]).unwrap();
        assert!(matches!(cli.command, Command::Eval { expr, .. } if expr == "-2^2"));

        let cli = Cli::try_parse_from(["hmflow-cli", "eval", "-a", "--var", "a=-3 m"]).unwrap();
        assert!(matches!(cli.command, Command::Eval { vars, .. } if vars == ["a=-3 m"]));

        let cli = Cli::try_parse_from(["hmflow-cli", "design", "--npsha", "-3 m"]).unwrap();
        assert!(matches!(cli.command, Command::Design { npsha: Some(text), .. } if text == "-3 m"));
    }

    #[test]
    fn new_options_round_trip_through_the_project() {
        // Un proyecto recién diseñado guarda las opciones del método v0.9 con
        // su valor por defecto («0» = sin dato) y no cambia el diseño.
        let mut project = designed_project();
        let (_, defaults) = design_inputs(&project).unwrap();
        assert_eq!(defaults.eta_v, None);
        assert!(!defaults.balance_holes);
        assert_eq!(defaults.volute_ratio, Some(hm_design::DEFAULT_VOLUTE_RATIO));
        assert_eq!(defaults.impeller_roughness, None);
        assert_eq!(defaults.volute_channels, Some(1));
        assert_eq!(defaults.volute_wrap_deg, None);
        assert!((defaults.incidence_deg.unwrap() - 2.0).abs() < 1e-9);

        // Y con valores manuales, el motor los usa.
        for (name, value) in [
            ("eta_v_fixed", "0.95"),
            ("balance_holes", "1"),
            ("volute_ratio", "0.85"),
            ("impeller_roughness", "0.00005 m"),
            ("volute_channels", "2"),
            ("volute_wrap_deg", "165 deg"),
            ("incidence_deg", "4 deg"),
        ] {
            project
                .params
                .insert(name.to_string(), manual(value.to_string()));
        }
        let (input, options) = design_inputs(&project).unwrap();
        assert_eq!(options.eta_v, Some(0.95));
        assert!(options.balance_holes);
        assert_eq!(options.volute_ratio, Some(0.85));
        assert_eq!(options.impeller_roughness, Some(0.00005));
        assert_eq!(options.volute_channels, Some(2));
        assert!((options.volute_wrap_deg.unwrap() - 165.0).abs() < 1e-9);
        assert!((options.incidence_deg.unwrap() - 4.0).abs() < 1e-9);

        let result = design_with_options(&input, &options).unwrap();
        assert_eq!(result.eta_v, 0.95);
        assert_eq!(result.selection.blade_count % 2, 1);
        assert!((result.volute.wrap_deg.unwrap() - 165.0).abs() < 1e-9);
        let c2 = result.volute.inlet_velocity.si;
        assert!((result.volute.throat_area.si - input.q.si / (0.85 * c2)).abs() < 1e-12);
    }

    #[test]
    fn zero_means_no_data_for_every_option_and_balance_holes_is_strict() {
        let mut project = designed_project();
        for (name, value) in [("volute_ratio", "0"), ("volute_channels", "0")] {
            project
                .params
                .insert(name.to_string(), manual(value.to_string()));
        }
        let (_, options) = design_inputs(&project).unwrap();
        assert_eq!(options.volute_ratio, None);
        assert_eq!(options.volute_channels, None);

        project
            .params
            .insert("balance_holes".to_string(), manual("0.5".to_string()));
        let error = design_inputs(&project).unwrap_err().to_string();
        assert!(error.contains("balance_holes"), "{error}");
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
        let diagnostics = audit(&project);
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
        let diagnostics = audit(&project);
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

    /// Proyecto completo tal como lo escribe `design` para la duty de ejemplo.
    fn designed_project() -> Project {
        let result = sample_design();
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = METHOD.to_string();
        project.operating.npsha = "15 m".to_string();
        let defaults = ManualDefaults::engine("998.2 kg/m3".to_string(), "0.96".to_string());
        project.params = design_params(&result, None, &defaults).unwrap().0;
        project
    }

    fn recalculated(project: &Project) -> Project {
        let (input, options) = design_inputs(project).unwrap();
        let result = design_with_options(&input, &options).unwrap();
        let defaults = ManualDefaults::engine("998.2 kg/m3".to_string(), "0.96".to_string());
        let mut project = project.clone();
        project.params = design_params(&result, Some(&project.params), &defaults)
            .unwrap()
            .0;
        project
    }

    #[test]
    fn recalc_keeps_manual_edits_and_clears_the_check() {
        let mut project = designed_project();
        project
            .params
            .insert("beta2".to_string(), manual("30 deg".to_string()));
        assert!(audit(&project).has_errors());

        let project = recalculated(&project);
        let diagnostics = audit(&project);
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
        let defaults = ManualDefaults::engine("998.2 kg/m3".to_string(), "0.96".to_string());
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
        let defaults = ManualDefaults::engine("998.2 kg/m3".to_string(), "0.96".to_string());
        assert!(design_params(&result, Some(&project.params), &defaults).is_err());
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
        let diagnostics = audit(&project);
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
        assert!(codes(&audit(&project)).contains(&"HM-DES-013"));
    }
}
