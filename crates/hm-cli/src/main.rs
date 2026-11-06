//! CLI de HidroMac Flow.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hm_bench::{BenchCase, Comparison, Level, Reduction, reduce};
use hm_core::Diagnostic;
use hm_engine::{
    ChangeKind, CurvePoints, Density, DensitySource, DesignOptions, DesignReport, Duty,
    EngineError, Entered, Fluid, Intake, PREFERRED_OPERATING_REGION, ParamChange, PumpType,
    analyze, audit, casing_fit, cli_message, compare_with_project, fluid_kind, looks_like_water,
    new_casing_project, new_project, parse_temperature, report, viscosity_from_text,
    water_density_at,
};
use hm_formula::{Registry, eval, infer, parse};
use hm_project::{Casing, Param, Project};
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
    /// Reduce un caso de ensayo de banco y, si indica su `.hmf`, lo compara
    /// con la curva del modelo.
    Bench {
        /// Ruta del archivo case.toml.
        file: PathBuf,
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
        /// Archivo con una tabla [casing]: diseña el rodete para esa carcasa.
        /// D2, ojo máximo, cubo, b3, lengua y canales pasan a ser datos.
        #[arg(long)]
        casing: Option<PathBuf>,
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

/// m³/s en un gpm.
const GPM: f64 = 3.785_411_784e-3 / 60.0;

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

        Command::Bench { file } => bench(&file),
        Command::Check { file } => {
            let project = Project::load(&file)?;
            let diagnostics = audit(&project);
            if diagnostics.is_empty() {
                println!("{}: sin diagnósticos", file.display());
            } else {
                print_diagnostics(diagnostics.iter());
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
            casing,
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
            let from_installation = suction_pressure.is_some();
            let intake = match suction_pressure {
                Some(pressure) => Intake::Installation {
                    pressure: Entered::parse(&pressure)?,
                    height: optional_entered(&suction_height.unwrap_or_default())?,
                    losses: optional_entered(&suction_losses.unwrap_or_default())?,
                    vapor_pressure: optional_entered(&vapor_pressure.unwrap_or_default())?,
                },
                None => match npsha.as_deref() {
                    Some(text) => Intake::Available(Entered::parse(text)?),
                    None => Intake::None,
                },
            };
            let duty = Duty {
                q: Entered::parse(&q)?,
                h: Entered::parse(&h)?,
                n: Entered::parse(&n)?,
            };
            // El error de la viscosidad se guarda: sale después de los de Q, H, n,
            // NPSHa y --curve-points, que son los que salían antes.
            let viscosity = viscosity_from_text(&viscosity.unwrap_or_default());

            // El rango se comprueba aquí, antes del análisis, con la misma frase.
            let Some(points) = CurvePoints::new(curve_points) else {
                return Err("--curve-points debe estar entre 2 y 1000".into());
            };

            let casing_text = casing
                .as_deref()
                .map(|path| -> Result<Casing, Box<dyn std::error::Error>> {
                    let text = std::fs::read_to_string(path)
                        .map_err(|error| format!("{}: {error}", path.display()))?;
                    Casing::from_toml(&text)
                        .map_err(|error| format!("{}: {error}", path.display()).into())
                })
                .transpose()?;
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
                casing: casing_text
                    .as_ref()
                    .map(casing_fit)
                    .transpose()
                    .map_err(engine_text)?,
                ..DesignOptions::default()
            };
            let case = hm_engine::Case {
                pump_type: PumpType::EndSuction,
                duty,
                fluid: Fluid {
                    kind: fluid_kind(&fluid),
                    temperature: Some(temperature_value),
                    density: Density {
                        value: rho_quantity,
                        source: DensitySource::Explicit { text: rho_text },
                    },
                    viscosity: viscosity.as_ref().ok().cloned().flatten(),
                },
                intake,
                mechanical_efficiency: eta_m,
                design: options,
            };
            let analysis = match analyze(&case, points) {
                Ok(analysis) => analysis,
                Err(error) => return design_failure(error),
            };
            if let Err(error) = viscosity {
                return Err(engine_text(error));
            }
            let input = &analysis.input;
            let result = &analysis.result;

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
            if let Some(model) = &result.model_volute {
                println!(
                    "  A360 = {:.6} m2   D360 = {:.4} m   R4 = {:.4} m",
                    model.throat_area.value(),
                    model.throat_diameter.value(),
                    model.outer_radius.value()
                );
            }
            if let Some(casing) = &result.casing {
                println!(
                    "  Carcasa: {} canal(es), cuello {:.3} mm, agujero {:.3} mm",
                    casing.channels,
                    casing.neck_diameter * 1000.0,
                    casing.bore_diameter * 1000.0
                );
            }
            println!(
                "  Pérdidas: hyd = {:.0} W   vol = {:.0} W   discos = {:.0} W   mech = {:.0} W",
                result.p_hyd_loss.value(),
                result.p_vol_loss.value(),
                result.p_disk_loss.value(),
                result.p_mech_loss.value()
            );
            // Sin garganta de la carcasa, Qc no se calcula: no es un límite real.
            let volute_limit = match (&result.casing, result.volute_matching_ratio(input.q.si)) {
                (Some(casing), None) if casing.throat_area.is_none() => "sin dato".to_string(),
                (Some(_), None) => "no limita".to_string(),
                _ => format!("{:.2}·Qd", result.volute_matching_flow.si / input.q.si),
            };
            println!(
                "  NPSH3: entrada sin choque {:.2}·Qd   voluta {}   subida desde {:.2}·Qd   recirculación ≈ {:.2}·Qd",
                result.shockless_flow.si / input.q.si,
                volute_limit,
                result.transition_ratio,
                result.recirculation_onset
            );
            let has_leakage = result
                .casing
                .as_ref()
                .is_some_and(|casing| casing.leakage.is_some());
            if has_leakage {
                println!(
                    "  Fugas por geometría: anillo delantero Qsp/Q {:.4}   trasero QE/Q {:.4}   ηv por fugas {:.4}   ηv del método {:.4}",
                    result.seal_leakage_ratio,
                    result.rear_seal_leakage_ratio,
                    result.seal_eta_v,
                    result.eta_v
                );
            } else {
                println!(
                    "  Contraste Gülich: ηv anillo delantero {:.3} (Qsp/Q {:.3}; ηv del método {:.3})   ηm {:.3} (motor {:.3})",
                    result.seal_eta_v,
                    result.seal_leakage_ratio,
                    result.eta_v,
                    result.mechanical_eta_estimate,
                    input.eta_m
                );
            }
            println!(
                "  Pérdidas Gülich: discos PRR/Pu {:.3}   ηh estadístico {:.3} (dimensionado {:.3})   ηv {:.3}   ηm {:.3}   → η {:.3}",
                result.disk_friction_ratio,
                result.statistical_eta_h,
                result.eta_h,
                result.leakage_eta_v,
                input.eta_m,
                result.eta
            );
            let shape = match &analysis.geometry {
                Ok(shape) => shape,
                Err(error) => return Err(error.to_string().into()),
            };
            println!(
                "  Álabe: β1B = {:.1} deg   envolvimiento = {:.1} deg",
                shape.impeller.beta1b.to_degrees(),
                shape.impeller.wrap_angle.to_degrees()
            );
            let cutwater = 2.0 * shape.outlet.cutwater_radius / result.d2.si;
            match &shape.outlet.model {
                Some(model) => println!(
                    "  Salida: lengua dz/d2 = {:.3}   DN {:.0}   difusor ≥ {:.3} m ({:.1} deg)",
                    cutwater,
                    model.nominal_diameter * 1000.0,
                    model.min_length,
                    model.half_angle.to_degrees()
                ),
                None => println!("  Salida: lengua dz/d2 = {cutwater:.3}"),
            }
            let mech = match &analysis.mechanics {
                Ok(mech) => mech,
                Err(error) => return Err(error.to_string().into()),
            };
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
                PREFERRED_OPERATING_REGION.1,
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
            if let (Some(npsha), true) = (&input.npsha, from_installation) {
                println!("  NPSHa de la instalación = {:.2} m", npsha.si);
            }

            let correction = match &analysis.viscous {
                Ok(correction) => correction,
                Err(error) => return Err(error.to_string().into()),
            };
            if let Some(correction) = correction {
                println!(
                    "  Viscosidad: ν = {:.3} cSt   fQ = {:.3}   fH = {:.3}   fη = {:.3}",
                    correction.nu * 1e6,
                    correction.f_q,
                    correction.f_h,
                    correction.f_eta
                );
            }
            print_diagnostics(&analysis.diagnostics);

            if analysis.diagnostics.iter().any(Diagnostic::is_error) {
                return Err("el diseño tiene errores; no se escribió el proyecto".into());
            }

            let path = out.unwrap_or_else(|| PathBuf::from(format!("{name}.hmf")));
            let curve_path = curves_out;

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

            // El análisis y la impresión ya ocurrieron. Aquí solo se escribe
            // el proyecto, con los mismos textos que se mostraron.
            let written = match &casing_text {
                Some(casing) => new_casing_project(&name, &case, &analysis, casing),
                None => new_project(&name, &case, &analysis),
            };
            let project = match written {
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

            if curves || curve_path.is_some() {
                println!("\n  Q (m3/h)   H (m)    eta     P_eje (kW)  NPSHr (m)");
                for point in &analysis.curve {
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
                    for point in &analysis.curve {
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

/// Recalcula los `auto` de un proyecto end-suction y lo reescribe.
///
/// El motor migra, rellena y recalcula. Aquí solo se carga, se guarda y se
/// imprimen los mismos avisos de antes.
fn recalc(file: &std::path::Path, reselect: bool) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let project = Project::load(file)?;
    match hm_engine::recalc(project, reselect) {
        Ok(done) => {
            print_diagnostics(done.diagnostics.iter());
            done.project.save(file)?;
            print_recalc_changes(&done.changes, stored_nq(&done.project));
            println!("recalculado {}", file.display());
            Ok(ExitCode::SUCCESS)
        }
        Err(EngineError::Invalid { diagnostics }) => {
            print_diagnostics(diagnostics.iter());
            Err("el proyecto tiene errores; no se recalculó".into())
        }
        Err(EngineError::DesignHasErrors { diagnostics }) => {
            print_diagnostics(diagnostics.iter());
            Err("el diseño tiene errores; no se escribió el proyecto".into())
        }
        Err(EngineError::RecalculatedInvalid { diagnostics }) => {
            print_diagnostics(diagnostics.iter());
            Err("el proyecto recalculado no valida; no se escribió".into())
        }
        // Los avisos del diseño salen antes de la causa, como siempre.
        Err(EngineError::AfterDesign { design, source }) => {
            print_diagnostics(design.iter());
            match *source {
                EngineError::RecalculatedInvalid { diagnostics } => {
                    print_diagnostics(diagnostics.iter());
                    Err("el proyecto recalculado no valida; no se escribió".into())
                }
                other => Err(engine_text(other)),
            }
        }
        Err(error @ EngineError::UnknownParameter { .. }) => Err(error.to_string().into()),
        Err(EngineError::Unsupported(pump_type)) => {
            Err(format!("recalc solo admite end-suction; el proyecto es {pump_type}").into())
        }
        Err(error) => Err(engine_text(error)),
    }
}

fn bench(file: &std::path::Path) -> Result<ExitCode, Box<dyn std::error::Error>> {
    let case = BenchCase::parse(&std::fs::read_to_string(file)?)?;
    let reduction = reduce(&case)?;
    print_reduction(&case, &reduction);

    if let Some(hmf) = &case.hmf {
        let path = file.parent().unwrap_or(std::path::Path::new(".")).join(hmf);
        let project = Project::load(&path)?;
        print_comparison(&compare_with_project(&reduction, &project)?, case.level);
    }
    Ok(ExitCode::SUCCESS)
}

/// Porcentaje con signo, o un guion si no hay dato.
fn signed_percent(value: Option<f64>) -> String {
    value.map_or("-".to_string(), |value| format!("{:+.1}", 100.0 * value))
}

/// Valor con su incertidumbre en porcentaje, o un guion si no hay dato.
fn with_uncertainty(value: Option<String>, uncertainty: Option<f64>) -> String {
    match (value, uncertainty) {
        (Some(value), Some(u)) => format!("{value} ±{:.1} %", 100.0 * u),
        (Some(value), None) => value,
        (None, _) => "-".to_string(),
    }
}

fn print_reduction(case: &BenchCase, reduction: &Reduction) {
    let level = match case.level {
        Level::Trial => "trial, no cuenta como validación",
        Level::Bench => "bench",
    };
    println!("caso {} ({level})", case.id);
    println!("referido a {:.0} rpm", reduction.speed * 60.0);
    for note in &reduction.notes {
        println!("aviso: {note}");
    }
    println!();
    println!("punto  Q* gpm  n rpm  lecturas  H* m            P* kW           η %");
    for point in &reduction.points {
        let u = point.uncertainty;
        println!(
            "{:>5}  {:>6.0}  {:>5}  {:>8}  {:<14}  {:<14}  {}",
            point.point,
            point.q / GPM,
            point
                .n
                .map_or("-".to_string(), |n| format!("{:.0}", n * 60.0)),
            point.readings,
            with_uncertainty(Some(format!("{:.2}", point.h)), u.map(|u| u.h)),
            with_uncertainty(
                point.p.map(|p| format!("{:.1}", p / 1000.0)),
                u.map(|u| u.p)
            ),
            with_uncertainty(
                point.eta.map(|eta| format!("{:.1}", 100.0 * eta)),
                u.and_then(|u| u.eta)
            ),
        );
    }
    if !reduction.npsh.is_empty() {
        println!();
        println!("Q* gpm  pasos  H ref m  NPSH3 m");
        for series in &reduction.npsh {
            println!(
                "{:>6.0}  {:>5}  {:>7.2}  {}",
                series.q / GPM,
                series.steps,
                series.h_reference,
                series
                    .npsh3
                    .map_or("no alcanzado".to_string(), |npsh3| format!("{npsh3:.2}")),
            );
        }
    }
}

fn print_comparison(comparison: &Comparison, level: Level) {
    println!();
    match level {
        Level::Trial => {
            println!("frente al modelo, (modelo − medido)/medido, en %: trial, sin veredicto");
            println!("punto  Q* gpm  H medida  H modelo    ΔH     Δη     ΔP");
        }
        Level::Bench => {
            println!("frente al modelo, (modelo − medido)/medido, en % (±5 % por verificar)");
            println!("punto  Q* gpm  H medida  H modelo    ΔH     Δη     ΔP  banda");
        }
    }
    for point in &comparison.points {
        println!(
            "{:>5}  {:>6.0}  {:>8.2}  {:>8.2}  {:>5}  {:>5}  {:>5}  {}",
            point.point,
            point.q / GPM,
            point.h_measured,
            point.h_model,
            signed_percent(Some(point.h_deviation)),
            signed_percent(point.eta_deviation),
            signed_percent(point.p_deviation),
            match (level, point.in_band) {
                (Level::Trial, _) => "",
                (Level::Bench, true) => "dentro",
                (Level::Bench, false) => "fuera",
            },
        );
    }
    if !comparison.outside.is_empty() {
        let outside: Vec<String> = comparison.outside.iter().map(u32::to_string).collect();
        println!("fuera de la curva del modelo: {}", outside.join(", "));
    }
    for series in &comparison.npsh {
        println!(
            "NPSH3 a {:.0} gpm: medido {:.2} m, modelo {:.2} m ({})",
            series.q / GPM,
            series.npsh3,
            series.npshr_model,
            signed_percent(Some(series.deviation)),
        );
    }
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
    let Some(points) = CurvePoints::new(curve_points) else {
        return Err("--curve-points debe estar entre 2 y 1000".into());
    };
    let project = Project::load(file)?;
    match report(&project, points) {
        Ok(report) => Ok(report),
        Err(EngineError::Invalid { diagnostics }) => {
            print_diagnostics(diagnostics.iter());
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

fn print_diagnostics<'a>(diagnostics: impl IntoIterator<Item = &'a Diagnostic>) {
    for diagnostic in diagnostics {
        println!(
            "[{:?}] {}: {}",
            diagnostic.severity, diagnostic.code, diagnostic.message
        );
        if let Some(hint) = &diagnostic.hint {
            println!("        sugerencia: {hint}");
        }
    }
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

/// Fallo de `analyze` en `design`, antes de imprimir el bloque.
///
/// Todo sale por stderr con la frase de antes (`error: entrada inválida: …`),
/// como el resto de los errores de la CLI. El código del diagnóstico queda en
/// el motor para quien lo necesite.
fn design_failure(error: EngineError) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match error {
        EngineError::Invalid { diagnostics } => {
            print_diagnostics(diagnostics.iter());
            Err("el diseño tiene errores; no se escribió el proyecto".into())
        }
        EngineError::Unsupported(pump_type) => {
            Err(format!("solo hay motor para end-suction; el proyecto es {pump_type}").into())
        }
        error => Err(engine_text(error)),
    }
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

    #[test]
    fn negative_expressions_and_values_parse() {
        let cli = Cli::try_parse_from(["hmflow-cli", "eval", "-2^2"]).unwrap();
        assert!(matches!(cli.command, Command::Eval { expr, .. } if expr == "-2^2"));

        let cli = Cli::try_parse_from(["hmflow-cli", "eval", "-a", "--var", "a=-3 m"]).unwrap();
        assert!(matches!(cli.command, Command::Eval { vars, .. } if vars == ["a=-3 m"]));

        let cli = Cli::try_parse_from(["hmflow-cli", "design", "--npsha", "-3 m"]).unwrap();
        assert!(matches!(cli.command, Command::Design { npsha: Some(text), .. } if text == "-3 m"));
    }
}
