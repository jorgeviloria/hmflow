//! CLI de HidroMac Flow.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hm_core::{Diagnostic, Diagnostics};
use hm_design::{
    DesignInput, DesignOptions, DesignResult, PARAM_SPECS, design, design_with_options,
    designed_si, param_spec, performance_curve, stored_si_matches,
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
        /// NPSH disponible, por ejemplo "10 m".
        #[arg(long, default_value = "10 m")]
        npsha: String,
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
                npsha: Some(parse_quantity(&npsha)?),
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
            project.operating.npsha = npsha_text;
            project.design.method = "end_suction_v0.1".to_string();

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

    let input = DesignInput {
        q,
        h,
        n,
        rho,
        eta_m,
        npsha,
    };

    let result = design_with_options(&input, &options)?;
    let mut diagnostics = result.diagnostics.clone();
    add_fluid_temperature_diagnostics(project, &input, &mut diagnostics);
    compare_stored_autos(&result, project, &mut diagnostics);
    Ok(Some(diagnostics))
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
        slip: param_f64(project, "slip", "1")?,
        cu2: param_f64(project, "cu2", "m/s")?,
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
                        "Vuelve a ejecutar el diseño para regenerar los parámetros automáticos.",
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
                        "Vuelve a ejecutar el diseño para regenerar los parámetros automáticos.",
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
}
