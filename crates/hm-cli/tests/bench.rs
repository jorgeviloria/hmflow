//! `hmflow-cli bench`, de punta a punta, con el caso ficticio de `hm-bench`.

use std::path::Path;
use std::process::{Command, Output};

const CASE: &str = include_str!("../../hm-bench/tests/fixtures/ejemplo.toml");

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hmflow-cli"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hmflow-bench-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_model(dir: &Path, speed: &str) {
    let output = run(
        dir,
        &[
            "design",
            "--q",
            "1060 gpm",
            "--h",
            "83 m",
            "--n",
            speed,
            "--out",
            "modelo.hmf",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn reduces_and_compares_with_the_model_next_to_the_case() {
    let dir = temp_dir("ok");
    make_model(&dir, "1750 rpm");
    std::fs::write(dir.join("case.toml"), CASE).unwrap();

    let output = run(&dir, &["bench", "case.toml"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("caso EJEMPLO-U1-2027-01 (bench)"), "{text}");
    assert!(text.contains("85.14 ±0.6 %"), "{text}");
    assert!(text.contains("frente al modelo"), "{text}");
    assert!(text.contains("no alcanzado"), "{text}");
    assert!(text.contains("fuera de la curva del modelo: 8"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_model_at_another_speed_is_an_error() {
    let dir = temp_dir("speed");
    make_model(&dir, "2900 rpm");
    std::fs::write(dir.join("case.toml"), CASE).unwrap();

    let output = run(&dir, &["bench", "case.toml"]);
    assert!(!output.status.success());
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("2900 rpm"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_incomplete_bench_case_is_an_error() {
    let dir = temp_dir("incomplete");
    let text = CASE.replace("authorization = true", "authorization = false");
    std::fs::write(dir.join("case.toml"), text).unwrap();

    let output = run(&dir, &["bench", "case.toml"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("authorization"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_trial_compares_but_gives_no_verdict() {
    let dir = temp_dir("trial");
    make_model(&dir, "1750 rpm");
    let text = CASE.replace("level = \"bench\"", "level = \"trial\"");
    std::fs::write(dir.join("case.toml"), text).unwrap();

    let output = run(&dir, &["bench", "case.toml"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(text.contains("sin veredicto"), "{text}");
    assert!(!text.contains("dentro"), "{text}");
    assert!(!text.contains("fuera\n"), "{text}");
    std::fs::remove_dir_all(&dir).ok();
}
