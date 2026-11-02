//! Puertas de calidad del repositorio.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use hm_units::{Quantity, parse_quantity, parse_unit};
use serde::Deserialize;

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_else(|| "ci".to_string());

    match task.as_str() {
        "ci" => ci(),
        "cases" => bool_to_code(cases()),
        "provenance" => provenance(),
        "licences" => licences(),
        "assets" => assets(),
        other => {
            eprintln!("tarea desconocida: {other}");
            eprintln!("tareas: ci, cases, provenance, licences, assets");
            ExitCode::FAILURE
        }
    }
}

fn bool_to_code(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn ci() -> ExitCode {
    if !cases() {
        return ExitCode::FAILURE;
    }

    let steps: Vec<(&str, Vec<&str>)> = vec![
        ("cargo", vec!["fmt", "--all", "--", "--check"]),
        (
            "cargo",
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("cargo", vec!["test", "--workspace"]),
    ];

    for (command, args) in steps {
        println!("$ {command} {}", args.join(" "));
        if !run(command, &args) {
            eprintln!("falló: {command} {}", args.join(" "));
            return ExitCode::FAILURE;
        }
    }

    println!("ci: ok");
    ExitCode::SUCCESS
}

fn provenance() -> ExitCode {
    let registry = hm_formula::Registry::all();
    let issues = registry.audit();

    if issues.is_empty() {
        let (pilot, reference, bench) = registry.validation_counts();
        println!(
            "provenance: {} fórmulas auditadas ({pilot} piloto, {reference} referencia, {bench} banco)",
            registry.len()
        );
        ExitCode::SUCCESS
    } else {
        for issue in &issues {
            eprintln!("{issue}");
        }
        eprintln!("provenance: {} problemas", issues.len());
        ExitCode::FAILURE
    }
}

fn licences() -> ExitCode {
    let required = [
        "LICENSE-APACHE",
        "LICENSE-MIT",
        "NOTICE",
        "ATTRIBUTION.md",
        "deny.toml",
    ];

    let mut ok = true;

    for file in required {
        if !Path::new(file).exists() {
            eprintln!("falta {file}");
            ok = false;
        }
    }

    let mut manifests = Vec::new();
    collect_manifests(Path::new("crates"), &mut manifests);
    manifests.push(PathBuf::from("xtask/Cargo.toml"));

    for manifest in manifests {
        let text = std::fs::read_to_string(&manifest).unwrap_or_default();
        if !text.contains("license.workspace = true") {
            eprintln!("{}: falta `license.workspace = true`", manifest.display());
            ok = false;
        }
    }

    if ok {
        println!("licences: ok");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn assets() -> ExitCode {
    let attribution = std::fs::read_to_string("ATTRIBUTION.md").unwrap_or_default();
    let mut files = Vec::new();

    if Path::new("assets").is_dir()
        && let Err(error) = walk(Path::new("assets"), &mut files)
    {
        eprintln!("no se pudo leer assets/: {error}");
        return ExitCode::FAILURE;
    }

    let mut ok = true;

    for file in files {
        let name = file
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();

        if !attribution.contains(name) {
            eprintln!("{}: no está declarado en ATTRIBUTION.md", file.display());
            ok = false;
        }
    }

    if ok {
        println!("assets: ok");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[derive(Debug, Deserialize)]
struct Recipe {
    id: String,
    title: String,
    formula: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Expected {
    value: f64,
    unit: String,
    tolerance: f64,
}

/// Ejecuta los casos de `tests/cases/`.
fn cases() -> bool {
    let directory = Path::new("tests/cases");
    if !directory.is_dir() {
        println!("cases: sin casos");
        return true;
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    entries.sort();

    let registry = hm_formula::Registry::all();
    let mut ok = true;
    let mut total = 0;

    for case in entries {
        total += 1;

        let recipe_text = std::fs::read_to_string(case.join("recipe.toml")).unwrap_or_default();
        let expected_text = std::fs::read_to_string(case.join("expected.toml")).unwrap_or_default();

        let recipe: Recipe = match toml::from_str(&recipe_text) {
            Ok(recipe) => recipe,
            Err(error) => {
                eprintln!("{}: recipe inválido: {error}", case.display());
                ok = false;
                continue;
            }
        };

        let expected: Expected = match toml::from_str(&expected_text) {
            Ok(expected) => expected,
            Err(error) => {
                eprintln!("{}: expected inválido: {error}", case.display());
                ok = false;
                continue;
            }
        };

        let inputs: Result<BTreeMap<String, Quantity>, _> = recipe
            .inputs
            .iter()
            .map(|(name, value)| parse_quantity(value).map(|quantity| (name.clone(), quantity)))
            .collect();

        let inputs = match inputs {
            Ok(inputs) => inputs,
            Err(error) => {
                eprintln!("{}: entrada inválida: {error}", case.display());
                ok = false;
                continue;
            }
        };

        let result = match registry.evaluate(&recipe.formula, &inputs) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("{}: no se pudo evaluar: {error}", case.display());
                ok = false;
                continue;
            }
        };

        let unit = match parse_unit(&expected.unit) {
            Ok(unit) => unit,
            Err(error) => {
                eprintln!("{}: unidad esperada inválida: {error}", case.display());
                ok = false;
                continue;
            }
        };

        let converted = match result.to(&unit) {
            Ok(converted) => converted,
            Err(error) => {
                eprintln!("{}: no se pudo convertir: {error}", case.display());
                ok = false;
                continue;
            }
        };

        let difference = (converted.value() - expected.value).abs();
        let passed = difference <= expected.tolerance;

        if passed {
            println!("ok   {}  {} ({})", recipe.id, recipe.title, recipe.source);
        } else {
            eprintln!(
                "FALLO {}  {}: {:.6} {} frente a {:.6} {} (tol {})",
                recipe.id,
                recipe.title,
                converted.value(),
                expected.unit,
                expected.value,
                expected.unit,
                expected.tolerance
            );
            ok = false;
        }
    }

    if ok {
        println!("cases: {total} casos");
    }

    ok
}

fn run(command: &str, args: &[&str]) -> bool {
    Command::new(command)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn collect_manifests(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_manifests(&path, out);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("Cargo.toml") {
            out.push(path);
        }
    }
}

fn walk(directory: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}
