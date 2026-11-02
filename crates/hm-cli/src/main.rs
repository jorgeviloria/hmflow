//! CLI de HidroMac Flow.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hm_core::{Diagnostic, Diagnostics};
use hm_design::{
    DEFAULT_K_EFF, DEFAULT_K3, DEFAULT_VOLUTE_C, DesignInput, DesignOptions, DesignResult,
    PARAM_SPECS, ParamSpec, design, design_with_options, designed_si, param_spec,
    performance_curve, stored_si_matches, table_selection,
};
use hm_formula::{Registry, eval, infer, parse};
use hm_project::{Param, Project, PumpType};
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
        /// Vuelve a tomar de la tabla las selecciones que dependen de nq
        /// (psi, phi, beta2, blade_count, c0, cm0) salvo las que estén en
        /// locked.
        #[arg(long)]
        reselect: bool,
    },
    /// Lista las fórmulas disponibles.
    Formulas,
    /// Evalúa una expresión con unidades.
    Eval {
        /// Expresión, por ejemplo "Q/(pi*d2*b2)".
        expr: String,
        /// Variable en formato nombre=valor, por ejemplo Q="120 m3/h".
        #[arg(long = "var", value_name = "NOMBRE=VALOR")]
        vars: Vec<String>,
    },
    /// Diseña preliminarmente una bomba end-suction OH1/OH2.
    Design {
        /// Nombre del proyecto y del archivo .hmf.
        #[arg(long, default_value = "diseno-end-suction")]
        name: String,
        /// Caudal de diseño, por ejemplo "120 m3/h".
        #[arg(long, default_value = "120 m3/h")]
        q: String,
        /// Altura de diseño, por ejemplo "45 m".
        #[arg(long, default_value = "45 m")]
        h: String,
        /// Velocidad de giro, por ejemplo "2900 rpm".
        #[arg(long, default_value = "2900 rpm")]
        n: String,
        /// Densidad del fluido, por ejemplo "998.2 kg/m3".
        #[arg(long, default_value = "998.2 kg/m3")]
        rho: String,
        /// Fluido; por defecto se deduce de la densidad.
        #[arg(long)]
        fluid: Option<String>,
        /// Temperatura del fluido.
        #[arg(long, default_value = "20 degC")]
        temperature: String,
        /// Eficiencia mecánica, mayor que 0 y menor que 1.
        #[arg(long, default_value_t = 0.96)]
        eta_m: f64,
        /// NPSH disponible de la instalación, por ejemplo "10 m". Sin él no
        /// se evalúa el margen de cavitación.
        #[arg(long)]
        npsha: Option<String>,
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
            let rho_text = rho.clone();
            let input = DesignInput {
                q: parse_quantity(&q)?,
                h: parse_quantity(&h)?,
                n: parse_quantity(&n)?,
                rho: parse_quantity(&rho)?,
                eta_m,
                npsha: npsha.as_deref().map(parse_quantity).transpose()?,
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
                "  NPSHr_d = {:.2} m   H0 = {:.2} m",
                result.npshr_design.value(),
                result.shutoff_head.value()
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
                "  Pérdidas: hyd = {:.0} W   vol = {:.0} W   mech = {:.0} W",
                result.p_hyd_loss.value(),
                result.p_vol_loss.value(),
                result.p_mech_loss.value()
            );

            for diagnostic in result.diagnostics.iter() {
                println!(
                    "[{:?}] {}: {}",
                    diagnostic.severity, diagnostic.code, diagnostic.message
                );
                if let Some(hint) = &diagnostic.hint {
                    println!("        sugerencia: {hint}");
                }
            }

            if result.diagnostics.has_errors() {
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
            project.operating.fluid = fluid.unwrap_or_else(|| {
                if (input.rho.si - 998.2).abs() < 10.0 {
                    "water".to_string()
                } else {
                    "custom".to_string()
                }
            });
            project.operating.temperature = temperature;
            project.operating.npsha = npsha_text.unwrap_or_default();
            project.design.method = "end_suction_v0.1".to_string();

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
    let rho = param_f64(project, "rho", "kg/m3")?
        .map(|value| Quantity::from_si(value, rho_unit))
        .unwrap_or(parse_quantity("998.2 kg/m3")?);
    let eta_m = param_f64(project, "eta_m", "1")?.unwrap_or(0.96);
    let npsha = if project.operating.npsha.trim().is_empty() {
        None
    } else {
        Some(parse_quantity(&project.operating.npsha)?)
    };
    let options = design_options(project)?;

    Ok((
        DesignInput {
            q,
            h,
            n,
            rho,
            eta_m,
            npsha,
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
    add_selection_diagnostics(project, &result, &mut diagnostics);
    compare_stored_autos(&result, project, &mut diagnostics);
    Ok(Some(diagnostics))
}

/// Selecciones que el motor toma de sus tablas según `nq`.
const NQ_SELECTIONS: [&str; 6] = ["psi", "phi", "beta2", "blade_count", "c0", "cm0"];

/// Valor de tabla, en SI, de una selección que depende de `nq`.
fn table_si(name: &str, nq: f64) -> Option<f64> {
    let table = table_selection(nq);
    Some(match name {
        "psi" => table.psi,
        "phi" => table.phi,
        "beta2" => table.beta2_deg.to_radians(),
        "blade_count" => f64::from(table.blade_count),
        "c0" => table.c0,
        "cm0" => table.cm0,
        _ => return None,
    })
}

/// Selecciones `manual` que ya no coinciden con la tabla para el `nq` actual.
///
/// `design` escribe como `manual` lo que eligió la tabla. Si después cambia
/// el punto de operación, esos valores quedan de otro `nq`. Las `locked` son
/// decisiones explícitas y no se señalan.
fn stale_selections(project: &Project, nq: f64) -> Vec<&'static str> {
    NQ_SELECTIONS
        .into_iter()
        .filter(|name| {
            let Some(Param::Manual { value }) = project.params.get(*name) else {
                return false;
            };
            let (Ok(quantity), Some(expected)) = (parse_quantity(value), table_si(name, nq)) else {
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
    let stale = stale_selections(project, result.nq);
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
                "{names} no {} con la tabla para nq = {:.2}; puede venir de otro punto de operación",
                if stale.len() == 1 { "coincide" } else { "coinciden" },
                result.nq
            ),
        )
        .with_hint(
            "Usa `hmflow-cli recalc --reselect <archivo>` para volver a la tabla, \
             o márcalas como locked si son decisiones tuyas.",
        ),
    );
}

/// Recalcula los `auto` de un proyecto end-suction y lo reescribe.
///
/// Conserva los parámetros manuales y bloqueados; los que falten se rellenan
/// con la selección del motor y se informa de cada uno. Con `reselect`, las
/// selecciones `manual` que dependen de `nq` se vuelven a tomar de la tabla.
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
    if !project.design.method.is_empty() && project.design.method != "end_suction_v0.1" {
        return Err(format!("método de diseño desconocido: {}", project.design.method).into());
    }
    if let Some(name) = project
        .params
        .keys()
        .find(|name| param_spec(name).is_none())
    {
        return Err(
            format!("parámetro desconocido: `{name}`; corrígelo antes de recalcular").into(),
        );
    }

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
    project.design.method = "end_suction_v0.1".to_string();

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
                "  `{name}`: {old} → {value} (tabla para nq = {:.2})",
                result.nq
            );
        }
    }
    for name in filled
        .iter()
        .filter(|name| !reselected.iter().any(|(old, _)| old == name))
    {
        let value = project.params.get(name).and_then(param_value).unwrap_or("");
        println!("  `{name}` no estaba en el proyecto; se usó {value}");
    }
    println!("recalculado {}", file.display());
    Ok(ExitCode::SUCCESS)
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
        "k3" => DEFAULT_K3.to_string(),
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

/// Parámetros del método end_suction_v0.1 para `result`.
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

fn add_fluid_temperature_diagnostics(
    project: &Project,
    input: &DesignInput,
    diagnostics: &mut Diagnostics,
) {
    if project.operating.fluid.eq_ignore_ascii_case("water") && (input.rho.si - 998.2).abs() > 10.0
    {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-009",
                format!(
                    "el fluido es `water` pero la densidad es {:.1} kg/m3; revisa fluid y rho",
                    input.rho.si
                ),
            )
            .with_hint("Usa fluid = \"custom\" o corrige la densidad."),
        );
    }

    if !project.operating.temperature.trim().is_empty()
        && let Ok(temperature) = parse_quantity(&project.operating.temperature)
        && (temperature.si - 293.15).abs() > 0.5
    {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-010",
                "la temperatura no ajusta la densidad en este modelo",
            )
            .with_hint("Comprueba que rho corresponda a la temperatura de operación."),
        );
    }
}

fn design_audit(project: &Project) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    if !project.design.method.is_empty() && project.design.method != "end_suction_v0.1" {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-016",
                format!("método de diseño desconocido: {}", project.design.method),
            )
            .with_hint("Métodos conocidos: end_suction_v0.1."),
        );
        return diagnostics;
    }

    if project.design.method == "end_suction_v0.1"
        && project.design.pump_type != PumpType::EndSuction
    {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-016",
                format!(
                    "el método end_suction_v0.1 no aplica a {}",
                    project.design.pump_type
                ),
            )
            .with_hint("Corrige el tipo de bomba o el método de diseño."),
        );
        return diagnostics;
    }

    if project.design.pump_type == PumpType::EndSuction
        && (project.design.method == "end_suction_v0.1" || !project.params.is_empty())
    {
        validate_design_schema(project, &mut diagnostics);
    }

    match recompute_design_diagnostics(project) {
        Ok(Some(design_diagnostics)) => {
            diagnostics.extend(design_diagnostics.into_vec());
        }
        Ok(None) => {}
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
                Diagnostic::error("HM-PROJ-016", format!("parámetro desconocido: `{name}`"))
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
            (Some(_), _) => diagnostics.push(Diagnostic::error(
                "HM-PROJ-018",
                format!("`{}` debe ser auto", spec.name),
            )),
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
                            "`{name}` guardado como «{text}» no coincide con el diseño recalculado ({:.6} {})",
                            shown.value(),
                            shown.unit.symbol
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
        assert!(diagnostics.is_empty());
    }

    fn sample_design() -> DesignResult {
        design(&DesignInput {
            q: parse_quantity("120 m3/h").unwrap(),
            h: parse_quantity("45 m").unwrap(),
            n: parse_quantity("2900 rpm").unwrap(),
            rho: parse_quantity("998.2 kg/m3").unwrap(),
            eta_m: 0.96,
            npsha: Some(parse_quantity("10 m").unwrap()),
        })
        .unwrap()
    }

    #[test]
    fn end_suction_method_on_barrel_is_reported() {
        let mut project = Project::new("demo", PumpType::Barrel);
        project.design.method = "end_suction_v0.1".to_string();

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
        assert!(codes(&diagnostics).contains(&"HM-PROJ-016"));
    }

    #[test]
    fn unknown_parameter_is_reported() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = "end_suction_v0.1".to_string();
        project
            .params
            .insert("d22".to_string(), manual("0.2 m".to_string()));

        let mut diagnostics = Diagnostics::new();
        validate_design_schema(&project, &mut diagnostics);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-016"));
    }

    #[test]
    fn missing_parameter_is_reported() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = "end_suction_v0.1".to_string();

        let mut diagnostics = Diagnostics::new();
        validate_design_schema(&project, &mut diagnostics);
        assert!(codes(&diagnostics).contains(&"HM-PROJ-017"));
    }

    #[test]
    fn wrong_formula_is_reported() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.design.method = "end_suction_v0.1".to_string();
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
        project.design.method = "end_suction_v0.1".to_string();
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
        project.design.method = "end_suction_v0.1".to_string();
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
    fn high_temperature_warns() {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.operating.temperature = "80 degC".to_string();

        let diagnostics = design_audit(&project);
        assert!(codes(&diagnostics).contains(&"HM-DES-010"));
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
        project.design.method = "end_suction_v0.1".to_string();
        project.operating.npsha = "10 m".to_string();
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
        project.operating.npsha = "15 m".to_string();
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
        assert_eq!(stale_selections(&project, 44.0), Vec::<&str>::new());
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

        assert_eq!(project.params.get("psi").and_then(param_value), Some("0.8"));
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
        moved_duty_project().save(&path).unwrap();

        recalc(&path, false).unwrap();
        let project = Project::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(
            project.params.get("psi").and_then(param_value),
            Some("0.87")
        );
        assert!(codes(&design_audit(&project)).contains(&"HM-DES-013"));
    }
}
