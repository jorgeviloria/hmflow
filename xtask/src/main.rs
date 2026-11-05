//! Puertas de calidad del repositorio.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use hm_core::{CODES, CodeStatus, Severity, code_info};
use hm_units::{Quantity, parse_quantity, parse_unit};
use serde::Deserialize;

fn main() -> ExitCode {
    let task = std::env::args().nth(1).unwrap_or_else(|| "ci".to_string());

    match task.as_str() {
        "ci" => ci(),
        "deps" => deps(),
        "cases" => bool_to_code(cases()),
        "provenance" => provenance(),
        "licences" => licences(),
        "assets" => assets(),
        "codes" => codes(std::env::args().nth(2).as_deref() == Some("--write")),
        other => {
            eprintln!("tarea desconocida: {other}");
            eprintln!("tareas: ci, deps, cases, provenance, licences, assets, codes [--write]");
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

/// Grafo de dependencias normales y de build: paquete → paquetes.
type Graph = BTreeMap<String, BTreeSet<String>>;

/// Capa de cada crate del workspace. Un arco hacia otro crate de esta tabla
/// solo es legal si el destino está en una capa estrictamente inferior.
/// Un crate del workspace que no esté aquí es un error. Un nombre que no esté
/// aquí es una dependencia externa y se ignora.
const LAYERS: &[(&str, u8)] = &[
    ("hm-core", 0),
    ("hm-units", 0),
    ("hm-math", 0),
    ("hm-formula", 1),
    ("hm-design", 2),
    ("hm-project", 2),
    ("hm-engine", 3),
    ("hmflow-cli", 4),
    ("hm-ui-egui", 4),
    ("xtask", 4),
];

fn layer_of(name: &str) -> Option<u8> {
    LAYERS
        .iter()
        .find_map(|(crate_name, layer)| (*crate_name == name).then_some(*layer))
}

fn deps() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."));

    let graph = match metadata_graph(root, None) {
        Ok(graph) => graph,
        Err(error) => {
            eprintln!("deps: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut issues = check_deps(&graph);

    let egui_manifest = root.join("crates/hm-ui-egui/Cargo.toml");
    if egui_manifest.is_file() {
        match metadata_graph(root, Some(&egui_manifest)) {
            Ok(egui_graph) => issues.extend(check_deps(&egui_graph)),
            Err(error) => {
                eprintln!("deps: hm-ui-egui: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    if issues.is_empty() {
        println!("deps: ok");
        ExitCode::SUCCESS
    } else {
        for issue in &issues {
            eprintln!("deps: {issue}");
        }
        eprintln!("deps: {} problemas", issues.len());
        ExitCode::FAILURE
    }
}

fn metadata_graph(root: &Path, manifest: Option<&Path>) -> Result<Graph, String> {
    let mut command = Command::new("cargo");
    command
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root);

    if let Some(manifest) = manifest {
        command.arg("--manifest-path").arg(manifest);
    }

    let output = command
        .output()
        .map_err(|error| format!("no se pudo ejecutar cargo metadata: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "cargo metadata falló: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    graph_from_metadata(&output.stdout)
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<PackageMeta>,
}

#[derive(Deserialize)]
struct PackageMeta {
    name: String,
    dependencies: Vec<DependencyMeta>,
}

#[derive(Deserialize)]
struct DependencyMeta {
    name: String,
    /// `null` en una dependencia normal, `"dev"` o `"build"` en las otras.
    kind: Option<String>,
}

fn graph_from_metadata(stdout: &[u8]) -> Result<Graph, String> {
    let metadata: Metadata = serde_json::from_slice(stdout)
        .map_err(|error| format!("cargo metadata no devolvió JSON válido: {error}"))?;

    let mut graph = Graph::new();
    for package in metadata.packages {
        let mut dependencies = BTreeSet::new();
        for dependency in package.dependencies {
            if dependency.kind.as_deref() == Some("dev") {
                continue;
            }
            dependencies.insert(dependency.name);
        }
        graph.insert(package.name, dependencies);
    }

    Ok(graph)
}

fn check_deps(graph: &Graph) -> Vec<String> {
    let mut issues = Vec::new();

    for (name, dependencies) in graph {
        let Some(from) = layer_of(name) else {
            issues.push(format!(
                "`{name}` es un crate del workspace y no está en la tabla de capas"
            ));
            continue;
        };

        for dependency in dependencies {
            let Some(to) = layer_of(dependency) else {
                continue;
            };
            if to >= from {
                issues.push(format!(
                    "`{name}` (capa {from}) no puede depender de `{dependency}` (capa {to}): el destino tiene que estar en una capa inferior"
                ));
            }
        }

        // Solo `hm-engine` junta el diseño con el proyecto para calcular. La
        // ventana también los ve, y solo para `Project::load` y `Project::save`:
        // no arma el caso desde el archivo. Otro adaptador estaría rehaciendo
        // el trabajo del motor.
        if name != "hm-engine"
            && name != "hm-ui-egui"
            && dependencies.contains("hm-design")
            && dependencies.contains("hm-project")
        {
            issues.push(format!(
                "`{name}` depende de `hm-design` y de `hm-project`: solo `hm-engine` junta el diseño con el proyecto"
            ));
        }
    }

    issues
}

fn ci() -> ExitCode {
    if deps() != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }
    if !cases() {
        return ExitCode::FAILURE;
    }
    if codes(false) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }
    // Las mismas comprobaciones que corre GitHub (.github/workflows/ci.yml).
    for check in [provenance, licences, assets] {
        if check() != ExitCode::SUCCESS {
            return ExitCode::FAILURE;
        }
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

/// Licencia del proyecto, tal como se declara en un `Cargo.toml`.
const PROJECT_LICENSE: &str = "license = \"MIT OR Apache-2.0\"";

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

    // Un crate con workspace propio (la interfaz egui) no puede heredar la
    // licencia: basta con que declare la misma de forma explícita.
    for manifest in manifests {
        let text = std::fs::read_to_string(&manifest).unwrap_or_default();
        if !text.contains("license.workspace = true") && !text.contains(PROJECT_LICENSE) {
            eprintln!(
                "{}: falta `license.workspace = true` o `{PROJECT_LICENSE}`",
                manifest.display()
            );
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

/// Ruta del catálogo de diagnósticos generado desde `hm_core::CODES`.
const DIAGNOSTICS_DOC: &str = "docs/diagnostics.md";

/// Comprueba el registro de códigos de diagnóstico:
///
/// - todo código citado en el código o en la documentación está registrado;
/// - todo código activo se emite en algún `src/`;
/// - ningún código retirado se emite;
/// - `docs/diagnostics.md` coincide con el registro (`--write` lo regenera).
fn codes(write: bool) -> ExitCode {
    let mut ok = true;
    let mut emitted: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut cited: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

    let mut files = Vec::new();
    for root in ["crates", "docs", "xtask/src"] {
        // `docs/` es local (no está en git): en un clon nuevo no existe.
        if !Path::new(root).exists() {
            continue;
        }
        if let Err(error) = walk(Path::new(root), &mut files) {
            eprintln!("no se pudo recorrer {root}: {error}");
            return ExitCode::FAILURE;
        }
    }
    files.push(PathBuf::from("README.md"));

    for path in files {
        let extension = path.extension().and_then(|e| e.to_str());
        if !matches!(extension, Some("rs" | "md" | "toml")) || path == Path::new(DIAGNOSTICS_DOC) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let in_src = path.components().any(|part| part.as_os_str() == "src")
            && path.starts_with("crates")
            && !path.ends_with("codes.rs");
        // `HM-XXX-` queda reservado para códigos inventados en tests.
        for code in find_codes(&text)
            .into_iter()
            .filter(|code| !code.starts_with("HM-XXX-"))
        {
            cited.entry(code.clone()).or_default().push(path.clone());
            if in_src && extension == Some("rs") {
                emitted.entry(code).or_default().push(path.clone());
            }
        }
    }

    for (code, paths) in &cited {
        match code_info(code) {
            None => {
                eprintln!(
                    "{code}: no está en hm_core::CODES (citado en {})",
                    paths[0].display()
                );
                ok = false;
            }
            Some(info) if info.status == CodeStatus::Retired && emitted.contains_key(code) => {
                eprintln!("{code}: está retirado y se sigue emitiendo");
                ok = false;
            }
            Some(_) => {}
        }
    }
    for info in CODES {
        if info.status == CodeStatus::Active && !emitted.contains_key(info.code) {
            eprintln!(
                "{}: activo en el registro pero no se emite en ningún src/",
                info.code
            );
            ok = false;
        }
    }

    let expected = diagnostics_markdown();
    let current = std::fs::read_to_string(DIAGNOSTICS_DOC).unwrap_or_default();
    // Sin `docs/` (clon nuevo) no hay catálogo que comparar, salvo que se pida
    // escribirlo.
    let docs_present = Path::new(DIAGNOSTICS_DOC)
        .parent()
        .is_some_and(Path::exists);
    if current != expected && (docs_present || write) {
        if write {
            if let Err(error) = std::fs::write(DIAGNOSTICS_DOC, &expected) {
                eprintln!("no se pudo escribir {DIAGNOSTICS_DOC}: {error}");
                return ExitCode::FAILURE;
            }
            println!("escrito {DIAGNOSTICS_DOC}");
        } else {
            eprintln!(
                "{DIAGNOSTICS_DOC} no coincide con hm_core::CODES; ejecuta `cargo xtask codes --write`"
            );
            ok = false;
        }
    }

    if ok {
        println!("codes: {} códigos registrados", CODES.len());
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Códigos `HM-XXX-NNN` de un texto.
fn find_codes(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(offset) = text[index..].find("HM-") {
        let start = index + offset;
        let mut cursor = start + 3;
        let letters = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_uppercase() {
            cursor += 1;
        }
        let valid_prefix = cursor > letters && cursor < bytes.len() && bytes[cursor] == b'-';
        if valid_prefix {
            let digits = cursor + 1;
            let end = digits + 3;
            if end <= bytes.len()
                && bytes[digits..end].iter().all(u8::is_ascii_digit)
                && bytes.get(end).is_none_or(|b| !b.is_ascii_digit())
            {
                found.push(text[start..end].to_string());
            }
        }
        index = start + 3;
    }
    found
}

fn diagnostics_markdown() -> String {
    let mut out = String::from(
        "# Códigos de diagnóstico\n\n\
         Generado desde `hm_core::CODES` con `cargo xtask codes --write`; no se edita a mano.\n\n\
         Un código tiene un solo significado y una sola severidad, y no se reutiliza nunca.\n\
         Ver `docs/adr/0003-versionado.md`.\n\n\
         | Código | Severidad | Estado | Significado |\n\
         |---|---|---|---|\n",
    );
    for info in CODES {
        let severity = match info.severity {
            Severity::Info => "info",
            Severity::Warning => "aviso",
            Severity::Error => "error",
            Severity::Blocking => "bloqueante",
        };
        let status = match info.status {
            CodeStatus::Active => "activo",
            CodeStatus::Retired => "retirado",
        };
        out.push_str(&format!(
            "| `{}` | {severity} | {status} | {} |\n",
            info.code, info.summary
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Graph, check_deps, find_codes, graph_from_metadata};
    use std::collections::{BTreeMap, BTreeSet};

    fn graph(entries: &[(&str, &[&str])]) -> Graph {
        entries
            .iter()
            .map(|(name, dependencies)| {
                (
                    (*name).to_string(),
                    dependencies
                        .iter()
                        .map(|name| (*name).to_string())
                        .collect(),
                )
            })
            .collect::<BTreeMap<_, BTreeSet<_>>>()
    }

    #[test]
    fn finds_well_formed_codes_only() {
        let text = "a HM-DES-007, b `HM-PROJ-025`; HM-DES-12 HM-des-001 HM-DES-0071 HM--001";
        assert_eq!(find_codes(text), ["HM-DES-007", "HM-PROJ-025"]);
    }

    #[test]
    fn a_graph_that_respects_the_layers_has_no_issues() {
        let graph = graph(&[
            ("hm-core", &["serde"]),
            ("hm-units", &[]),
            ("hm-math", &[]),
            ("hm-formula", &["hm-units"]),
            (
                "hm-design",
                &["hm-core", "hm-formula", "hm-math", "hm-units"],
            ),
            ("hm-project", &["hm-core", "hm-formula", "hm-units"]),
            (
                "hm-engine",
                &[
                    "hm-core",
                    "hm-design",
                    "hm-formula",
                    "hm-project",
                    "hm-units",
                ],
            ),
            (
                "hmflow-cli",
                &[
                    "hm-core",
                    "hm-engine",
                    "hm-formula",
                    "hm-project",
                    "hm-units",
                ],
            ),
            (
                "hm-ui-egui",
                &[
                    "hm-core",
                    "hm-design",
                    "hm-engine",
                    "hm-formula",
                    "hm-project",
                    "hm-units",
                ],
            ),
            ("xtask", &["hm-core", "hm-formula", "hm-units"]),
        ]);

        let issues = check_deps(&graph);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn the_domain_cannot_depend_on_persistence() {
        let issues = check_deps(&graph(&[("hm-design", &["hm-project"])]));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("hm-design") && issues[0].contains("hm-project"));
    }

    #[test]
    fn a_lower_layer_cannot_depend_on_a_higher_one() {
        let issues = check_deps(&graph(&[("hm-formula", &["hm-design"])]));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("hm-formula") && issues[0].contains("hm-design"));
    }

    #[test]
    fn only_the_engine_joins_design_and_project() {
        let issues = check_deps(&graph(&[(
            "hmflow-cli",
            &["hm-engine", "hm-design", "hm-project"],
        )]));
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].contains("hmflow-cli") && issues[0].contains("hm-engine"));

        let engine = check_deps(&graph(&[("hm-engine", &["hm-design", "hm-project"])]));
        assert!(engine.is_empty(), "{engine:?}");

        // La ventana carga y guarda; no es el motor.
        let window = check_deps(&graph(&[(
            "hm-ui-egui",
            &["hm-engine", "hm-design", "hm-project"],
        )]));
        assert!(window.is_empty(), "{window:?}");

        // Cada uno por separado sí puede.
        let separate = check_deps(&graph(&[
            ("hmflow-cli", &["hm-engine", "hm-project"]),
            ("hm-ui-egui", &["hm-engine", "hm-design"]),
        ]));
        assert!(separate.is_empty(), "{separate:?}");
    }

    #[test]
    fn the_engine_cannot_depend_on_an_adapter() {
        let issues = check_deps(&graph(&[("hm-engine", &["hmflow-cli"])]));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("hm-engine") && issues[0].contains("hmflow-cli"));
    }

    #[test]
    fn the_domain_and_persistence_cannot_depend_on_the_engine() {
        let issues = check_deps(&graph(&[
            ("hm-design", &["hm-engine"]),
            ("hm-project", &["hm-engine"]),
        ]));
        assert_eq!(issues.len(), 2);
    }

    #[test]
    fn the_cli_and_the_ui_cannot_depend_on_each_other() {
        let issues = check_deps(&graph(&[
            ("hmflow-cli", &["hm-ui-egui"]),
            ("hm-ui-egui", &["hmflow-cli"]),
        ]));
        assert_eq!(issues.len(), 2);
    }

    #[test]
    fn the_ui_cannot_depend_on_the_cli() {
        let issues = check_deps(&graph(&[("hm-ui-egui", &["hmflow-cli"])]));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("hm-ui-egui") && issues[0].contains("hmflow-cli"));
    }

    #[test]
    fn a_workspace_crate_missing_from_the_table_is_an_error() {
        let issues = check_deps(&graph(&[("hm-nuevo", &[])]));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("hm-nuevo"));
    }

    #[test]
    fn metadata_without_a_package_name_is_rejected() {
        let error = graph_from_metadata(br#"{"packages":[{"dependencies":[]}]}"#).unwrap_err();
        assert!(error.contains("JSON"), "{error}");
    }

    #[test]
    fn metadata_keeps_normal_and_build_dependencies() {
        let graph = graph_from_metadata(
            br#"{"packages":[{"name":"hm-formula","dependencies":[
                {"name":"hm-units","kind":null},
                {"name":"serde","kind":"dev"},
                {"name":"cc","kind":"build"}
            ]}]}"#,
        )
        .unwrap();
        let dependencies = &graph["hm-formula"];
        assert!(dependencies.contains("hm-units"));
        assert!(dependencies.contains("cc"));
        assert!(!dependencies.contains("serde"));
    }
}
