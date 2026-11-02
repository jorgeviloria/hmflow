//! CLI de HidroMac Flow.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hm_design::{DesignInput, design, performance_curve};
use hm_formula::{Registry, eval, infer, parse};
use hm_project::{Param, Project, PumpType};
use hm_units::{Quantity, parse_quantity};

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
        /// Eficiencia mecánica entre 0 y 1.
        #[arg(long, default_value_t = 0.96)]
        eta_m: f64,
        /// NPSH disponible, por ejemplo "6 m".
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
                None
            } else {
                match recompute_design_diagnostics(&project) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!("no se pudo recalcular el diseño: {error}");
                        None
                    }
                }
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
            if let Some(design_diagnostics) = &design_diagnostics {
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
            }
            if !printed {
                println!("{}: sin diagnósticos", file.display());
            }

            let design_has_errors = design_diagnostics
                .as_ref()
                .is_some_and(hm_core::Diagnostics::has_errors);

            if diagnostics.has_errors() || design_has_errors {
                Ok(ExitCode::FAILURE)
            } else {
                Ok(ExitCode::SUCCESS)
            }
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

            let mut params = BTreeMap::new();
            params.insert("rho".to_string(), manual(rho_text));
            params.insert("eta_m".to_string(), manual(eta_m.to_string()));
            params.insert("mm".to_string(), manual("0.001 m".to_string()));
            params.insert("psi".to_string(), manual(result.selection.psi.to_string()));
            params.insert("phi".to_string(), manual(result.selection.phi.to_string()));
            params.insert(
                "beta2".to_string(),
                manual(format!("{} deg", result.selection.beta2_deg)),
            );
            params.insert(
                "blade_count".to_string(),
                manual(result.selection.blade_count.to_string()),
            );
            params.insert(
                "c0".to_string(),
                manual(format!("{} m/s", result.selection.c0)),
            );
            params.insert(
                "cm0".to_string(),
                manual(format!("{} m/s", result.selection.cm0)),
            );
            params.insert("A".to_string(), manual(result.selection.a.to_string()));
            params.insert("J".to_string(), manual(result.selection.j.to_string()));
            params.insert(
                "margin".to_string(),
                manual(result.selection.margin.to_string()),
            );
            params.insert(
                "nq".to_string(),
                auto("similarity.nq", format!("{:.6}", result.nq)),
            );
            params.insert(
                "dn".to_string(),
                auto(
                    "design.specific_diameter",
                    format!("{:.4}", result.specific_diameter_mm),
                ),
            );
            params.insert(
                "sigma".to_string(),
                auto("design.thoma_sigma", format!("{:.6}", result.sigma)),
            );
            params.insert(
                "d0".to_string(),
                auto("design.d0", format!("{} m", result.d0.value())),
            );
            params.insert(
                "d1".to_string(),
                auto("design.d1", format!("{} m", result.d1.value())),
            );
            params.insert(
                "b1".to_string(),
                auto("design.b1", format!("{} m", result.b1.value())),
            );
            params.insert(
                "u2".to_string(),
                auto("design.u2", format!("{} m/s", result.u2.value())),
            );
            params.insert(
                "d2".to_string(),
                auto("design.d2", format!("{} m", result.d2.value())),
            );
            params.insert(
                "qth".to_string(),
                auto("design.qth", format!("{} m3/s", result.qth.value())),
            );
            params.insert(
                "cm2".to_string(),
                auto("design.cm2", format!("{} m/s", result.cm2.value())),
            );
            params.insert(
                "b2".to_string(),
                auto("design.b2", format!("{} m", result.b2.value())),
            );
            params.insert(
                "eta_h".to_string(),
                auto(
                    "design.hydraulic_efficiency",
                    format!("{:.6}", result.eta_h),
                ),
            );
            params.insert(
                "eta_v".to_string(),
                auto(
                    "design.volumetric_efficiency",
                    format!("{:.6}", result.eta_v),
                ),
            );
            params.insert(
                "eta".to_string(),
                auto("design.overall_efficiency", format!("{:.6}", result.eta)),
            );
            params.insert(
                "p_hyd".to_string(),
                auto(
                    "perf.hydraulic_power",
                    format!("{} W", result.p_hyd.value()),
                ),
            );
            params.insert(
                "p_shaft".to_string(),
                auto(
                    "design.shaft_power",
                    format!("{} W", result.p_shaft.value()),
                ),
            );
            params.insert(
                "torque".to_string(),
                auto(
                    "mech.shaft_torque",
                    format!("{} N*m", result.torque.value()),
                ),
            );
            params.insert(
                "p_max".to_string(),
                auto("design.max_power", format!("{} W", result.p_max.value())),
            );
            params.insert(
                "p_hyd_loss".to_string(),
                auto(
                    "design.hydraulic_loss",
                    format!("{} W", result.p_hyd_loss.value()),
                ),
            );
            params.insert(
                "p_vol_loss".to_string(),
                auto(
                    "design.volumetric_loss",
                    format!("{} W", result.p_vol_loss.value()),
                ),
            );
            params.insert(
                "p_mech_loss".to_string(),
                auto(
                    "design.mechanical_loss",
                    format!("{} W", result.p_mech_loss.value()),
                ),
            );
            params.insert(
                "npshr_d".to_string(),
                auto(
                    "design.npshr_design",
                    format!("{} m", result.npshr_design.value()),
                ),
            );
            params.insert(
                "h0".to_string(),
                auto(
                    "design.shutoff_head",
                    format!("{} m", result.shutoff_head.value()),
                ),
            );
            params.insert("k3".to_string(), manual("1.04".to_string()));
            params.insert("volute_c".to_string(), manual("0.05".to_string()));
            params.insert("k_eff".to_string(), manual("1.1".to_string()));
            params.insert("slip".to_string(), manual(result.slip.to_string()));
            params.insert(
                "cu2".to_string(),
                manual(format!("{} m/s", result.cu2.value())),
            );
            params.insert(
                "r3".to_string(),
                auto(
                    "design.volute_base_radius",
                    format!("{} m", result.volute.base_radius.value()),
                ),
            );
            params.insert(
                "b3".to_string(),
                auto(
                    "design.volute_width",
                    format!("{} m", result.volute.width.value()),
                ),
            );
            params.insert(
                "c3".to_string(),
                auto(
                    "design.volute_inlet_velocity",
                    format!("{} m/s", result.volute.inlet_velocity.value()),
                ),
            );
            params.insert(
                "a360".to_string(),
                auto(
                    "design.volute_throat_area",
                    format!("{} m2", result.volute.throat_area.value()),
                ),
            );
            params.insert(
                "d360".to_string(),
                auto(
                    "design.volute_throat_diameter",
                    format!("{} m", result.volute.throat_diameter.value()),
                ),
            );
            params.insert(
                "r4".to_string(),
                auto(
                    "design.volute_outer_radius",
                    format!("{} m", result.volute.outer_radius.value()),
                ),
            );
            project.params = params;

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

fn recompute_design_diagnostics(
    project: &Project,
) -> Result<Option<hm_core::Diagnostics>, Box<dyn std::error::Error>> {
    if project.design.pump_type != PumpType::EndSuction {
        return Ok(None);
    }

    let q = parse_quantity(&project.operating.q)?;
    let h = parse_quantity(&project.operating.h)?;
    let n = parse_quantity(&project.operating.n)?;
    let rho = project
        .params
        .get("rho")
        .and_then(param_value)
        .map(parse_quantity)
        .transpose()?
        .unwrap_or(parse_quantity("998.2 kg/m3")?);
    let eta_m = project
        .params
        .get("eta_m")
        .and_then(param_value)
        .map(parse_quantity)
        .transpose()?
        .map(|value| value.si)
        .unwrap_or(0.96);
    let npsha = if project.operating.npsha.trim().is_empty() {
        None
    } else {
        Some(parse_quantity(&project.operating.npsha)?)
    };

    let input = DesignInput {
        q,
        h,
        n,
        rho,
        eta_m,
        npsha,
    };

    Ok(Some(design(&input)?.diagnostics))
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
