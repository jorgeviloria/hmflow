//! `geometry`: exporta la vista en planta del proyecto, la misma del visor.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hm_project::Project;

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hmflow-cli"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

/// Carpeta temporal con un proyecto actual y uno de un método anterior.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hmflow-geometry-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../hm-project/tests/fixtures");
    std::fs::copy(
        fixtures.join("end_suction_v0.9.hmf"),
        dir.join("rodete.hmf"),
    )
    .unwrap();
    std::fs::copy(fixtures.join("end_suction_v0.8.hmf"), dir.join("viejo.hmf")).unwrap();
    dir
}

/// El CSV y la huella que da el motor para `rodete.hmf`.
fn expected(dir: &Path) -> (String, String) {
    let plan = hm_engine::plan(&Project::load(&dir.join("rodete.hmf")).unwrap()).unwrap();
    (plan.to_csv(), plan.fingerprint_text())
}

#[test]
fn the_file_is_the_plan_of_the_project() {
    let dir = temp_dir("archivo");
    let output = run(&dir, &["geometry", "rodete.hmf", "--out", "curvas.csv"]);
    assert!(output.status.success(), "{output:?}");
    let (csv, fingerprint) = expected(&dir);
    assert_eq!(
        std::fs::read_to_string(dir.join("curvas.csv")).unwrap(),
        csv
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&fingerprint), "{stderr}");
    assert!(output.stdout.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn without_out_it_goes_to_standard_output() {
    let dir = temp_dir("salida");
    let output = run(&dir, &["geometry", "rodete.hmf"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected(&dir).0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_existing_file_needs_force() {
    let dir = temp_dir("force");
    std::fs::write(dir.join("curvas.csv"), "mío").unwrap();
    let output = run(&dir, &["geometry", "rodete.hmf", "--out", "curvas.csv"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--force"));
    assert_eq!(
        std::fs::read_to_string(dir.join("curvas.csv")).unwrap(),
        "mío"
    );

    let output = run(
        &dir,
        &["geometry", "rodete.hmf", "--out", "curvas.csv", "--force"],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join("curvas.csv")).unwrap(),
        expected(&dir).0
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_old_method_is_refused_and_writes_nothing() {
    let dir = temp_dir("viejo");
    let output = run(&dir, &["geometry", "viejo.hmf", "--out", "curvas.csv"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("recalc"),
        "{output:?}"
    );
    assert!(!dir.join("curvas.csv").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_meridian_view_exports_the_section() {
    let dir = temp_dir("meridiano");
    let output = run(&dir, &["geometry", "rodete.hmf", "--view", "meridian"]);
    assert!(output.status.success(), "{output:?}");
    let model = hm_engine::meridian(&Project::load(&dir.join("rodete.hmf")).unwrap()).unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), model.to_csv());
    assert!(String::from_utf8_lossy(&output.stderr).contains(&model.fingerprint_text()));

    let output = run(&dir, &["geometry", "rodete.hmf", "--view", "alzado"]);
    assert!(!output.status.success());
    std::fs::remove_dir_all(&dir).ok();
}
