//! CLI de HidroMac Flow.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hm_core::Diagnostics;
#[cfg(test)]
use hm_design::METHOD;
use hm_design::geometry::geometry;
use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics, mechanics_diagnostics};
use hm_design::{
    DesignInput, DesignOptions, DesignReport, default_mechanical_efficiency, design_with_options,
    performance_curve,
};
use hm_engine::{
    ChangeKind, CurvePoints, Density, DensitySource, EngineError, Entered, Fluid, Intake,
    ParamChange, audit, cli_message, fluid_diagnostics, fluid_kind, kinematic_viscosity,
    looks_like_water, new_project, npsh_fluid_diagnostics, parse_temperature, report,
    viscosity_effect, viscosity_from_text, water_density_at,
};
#[cfg(test)]
use hm_engine::{
    MAX_COUNT, ManualDefaults, Temperature, design_params, kinematic_viscosity_of, suction_npsha,
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

            // El cálculo y la impresión ya ocurrieron. Aquí solo se escribe
            // el proyecto, con los mismos textos que se mostraron.
            let analysis = hm_engine::Analysis {
                input: input.clone(),
                result: result.clone(),
                curve: Vec::new(),
                geometry: Ok(shape.clone()),
                mechanics: Ok(mech.clone()),
                viscous: Ok(correction.clone()),
                diagnostics: Vec::new(),
            };
            let case = hm_engine::Case {
                pump_type: PumpType::EndSuction,
                duty: hm_engine::Duty {
                    q: Entered {
                        text: q,
                        quantity: input.q.clone(),
                    },
                    h: Entered {
                        text: h,
                        quantity: input.h.clone(),
                    },
                    n: Entered {
                        text: n,
                        quantity: input.n.clone(),
                    },
                },
                fluid: case_fluid,
                intake,
                mechanical_efficiency: Some(eta_m),
                design: options,
            };
            let project = match new_project(&name, &case, &analysis) {
                Ok(project) => project,
                Err(EngineError::Invalid { diagnostics }) => {
                    for diagnostic in diagnostics.iter() {
                        eprintln!(
                            "[{:?}] {}: {}",
                            diagnostic.severity, diagnostic.code, diagnostic.message
                        );
                    }
                    return Err("el proyecto generado no valida; no se escribió".into());
                }
                Err(error) => return Err(engine_text(error)),
            };

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
///
/// Solo la usan las pruebas que leen las opciones desde el `.hmf`.
#[cfg(test)]
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

/// Recalcula los `auto` de un proyecto end-suction y lo reescribe.
///
/// El motor migra, rellena y recalcula. Aquí solo se carga, se guarda y se
/// imprimen los mismos avisos de antes.
fn recalc(file: &std::path::Path, reselect: bool) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let project = Project::load(file)?;
    let prior_invalid = project.validate(&Registry::all()).has_errors();
    match hm_engine::recalc(project, reselect) {
        Ok(done) => {
            print_diagnostics(&done.diagnostics);
            done.project.save(file)?;
            print_recalc_changes(&done.changes, stored_nq(&done.project));
            println!("recalculado {}", file.display());
            Ok(ExitCode::SUCCESS)
        }
        Err(EngineError::Invalid { diagnostics }) => {
            Err(recalc_invalid(prior_invalid, &diagnostics))
        }
        Err(EngineError::Unsupported(pump_type)) => {
            Err(format!("recalc solo admite end-suction; el proyecto es {pump_type}").into())
        }
        Err(error) => Err(engine_text(error)),
    }
}

/// Las tres frases con las que `recalc` se niega a escribir, más el
/// parámetro desconocido, que sale solo como mensaje.
fn recalc_invalid(prior_invalid: bool, diagnostics: &Diagnostics) -> Box<dyn std::error::Error> {
    if !prior_invalid
        && diagnostics.len() == 1
        && diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-PROJ-023")
    {
        return diagnostics
            .iter()
            .next()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_default()
            .into();
    }
    print_diagnostics(diagnostics);
    let sentence = if prior_invalid {
        "el proyecto tiene errores; no se recalculó"
    } else if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error() && diagnostic.code.starts_with("HM-DES"))
    {
        "el diseño tiene errores; no se escribió el proyecto"
    } else {
        "el proyecto recalculado no valida; no se escribió"
    };
    sentence.into()
}

fn print_recalc_changes(changes: &[ParamChange], nq: f64) {
    for change in changes {
        match change.kind {
            ChangeKind::Reselected => println!(
                "  `{}`: {} → {} (selección por defecto para nq = {nq:.2})",
                change.name,
                change.before.as_deref().unwrap_or(""),
                change.after
            ),
            ChangeKind::Filled => println!(
                "  `{}` no estaba en el proyecto; se usó {}",
                change.name, change.after
            ),
            ChangeKind::MechanicalEfficiency => println!(
                "  `eta_m`: {} → {} (desde end_suction_v0.4 no incluye el rozamiento de discos)",
                change.before.as_deref().unwrap_or(""),
                change.after
            ),
            ChangeKind::MethodMigrated => println!(
                "  método migrado de {} a {}",
                change.before.as_deref().unwrap_or(""),
                change.after
            ),
        }
    }
}

/// nq guardado por el recálculo. La línea de `--reselect` lo imprime con dos
/// decimales; el texto adimensional da la misma vuelta que el número del motor.
fn stored_nq(project: &Project) -> f64 {
    project
        .params
        .get("nq")
        .and_then(param_value)
        .and_then(|text| parse_quantity(text).ok())
        .map(|quantity| quantity.si)
        .unwrap_or(f64::NAN)
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
    let points = CurvePoints::new(curve_points).expect("2..=1000 ya se comprobó");
    match report(&project, points) {
        Ok(report) => Ok(report),
        Err(EngineError::Invalid { diagnostics }) => {
            print_diagnostics(&diagnostics);
            Err("el proyecto tiene errores; no hay informe".into())
        }
        Err(EngineError::Unsupported(pump_type)) => {
            Err(format!("solo hay informe de end-suction; el proyecto es {pump_type}").into())
        }
        Err(EngineError::LegacyMethod { method }) => Err(format!(
            "el proyecto es de {method}; migra con `hmflow-cli recalc` antes de pedir el informe"
        )
        .into()),
        Err(error) => Err(engine_text(error)),
    }
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn param_u32(project: &Project, name: &str) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let Some(value) = param_f64(project, name, "1")? else {
        return Ok(None);
    };
    if value.fract() != 0.0 || !(1.0..=MAX_COUNT).contains(&value) {
        return Err(format!("`{name}` debe ser un entero entre 1 y {MAX_COUNT}").into());
    }
    Ok(Some(value as u32))
}

/// Densidad que se asume si el proyecto no guarda `rho`: la del agua a su
/// temperatura (20 °C si no la indica), o 998,2 kg/m³ si no es agua o la
/// temperatura cae fuera de la correlación.
///
/// Un texto de temperatura ilegible no aborta: se usa la densidad por defecto,
/// igual que antes de pasar el cálculo al motor.
#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use hm_design::{DesignResult, design};

    fn manual(value: String) -> Param {
        Param::Manual { value }
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
}
