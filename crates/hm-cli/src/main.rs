//! CLI de HidroMac Flow.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hm_design::{DesignInput, design};
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
        Command::New { name, kind } => {
            if name.contains('/') || name.contains('\\') {
                return Err("el nombre no puede contener separadores de ruta".into());
            }

            let pump_type = PumpType::parse(&kind)
                .ok_or_else(|| format!("tipo de bomba desconocido: {kind}"))?;

            let path = PathBuf::from(format!("{name}.hmf"));
            if path.exists() {
                return Err(format!("ya existe {}", path.display()).into());
            }

            let project = Project::new(&name, pump_type);
            project.save(&path)?;
            println!("creado {}", path.display());
            Ok(ExitCode::SUCCESS)
        }

        Command::Check { file } => {
            let project = Project::load(&file)?;
            let diagnostics = project.validate(&Registry::all());

            if diagnostics.is_empty() {
                println!("{}: sin diagnósticos", file.display());
            } else {
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

            if diagnostics.has_errors() {
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
            eta_m,
            npsha,
            out,
            force,
        } => {
            if name.contains('/') || name.contains('\\') {
                return Err("el nombre no puede contener separadores de ruta".into());
            }

            let npsha_text = npsha.clone();
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

            for diagnostic in result.diagnostics.iter() {
                println!(
                    "[{:?}] {}: {}",
                    diagnostic.severity, diagnostic.code, diagnostic.message
                );
                if let Some(hint) = &diagnostic.hint {
                    println!("        sugerencia: {hint}");
                }
            }

            let path = out.unwrap_or_else(|| PathBuf::from(format!("{name}.hmf")));
            if path.exists() && !force {
                return Err(format!(
                    "ya existe {}; usa --force para sobrescribir",
                    path.display()
                )
                .into());
            }

            let mut project = Project::new(&name, PumpType::EndSuction);
            project.operating.q = q;
            project.operating.h = h;
            project.operating.n = n;
            project.operating.fluid = "water".to_string();
            project.operating.npsha = npsha_text.unwrap_or_default();

            let mut params = BTreeMap::new();
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
            params.insert("nq".to_string(), auto("similarity.nq"));
            params.insert("d0".to_string(), auto("design.d0"));
            params.insert("d1".to_string(), auto("design.d1"));
            params.insert("b1".to_string(), auto("design.b1"));
            params.insert("u2".to_string(), auto("design.u2"));
            params.insert("d2".to_string(), auto("design.d2"));
            params.insert("qth".to_string(), auto("design.qth"));
            params.insert("cm2".to_string(), auto("design.cm2"));
            params.insert("b2".to_string(), auto("design.b2"));
            params.insert("eta_h".to_string(), auto("design.hydraulic_efficiency"));
            params.insert("eta_v".to_string(), auto("design.volumetric_efficiency"));
            params.insert("eta".to_string(), auto("design.overall_efficiency"));
            params.insert("p_hyd".to_string(), auto("perf.hydraulic_power"));
            params.insert("p_shaft".to_string(), auto("design.shaft_power"));
            params.insert("torque".to_string(), auto("mech.shaft_torque"));
            params.insert("p_max".to_string(), auto("design.max_power"));
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
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn manual(value: String) -> Param {
    Param::Manual { value }
}

fn auto(formula: &str) -> Param {
    Param::Auto {
        formula: formula.to_string(),
    }
}
