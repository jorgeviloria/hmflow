//! `design --casing`, de punta a punta, con una carcasa ficticia.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CASING: &str = "[casing]
d2 = \"210 mm\"
neck_diameter = \"90 mm\"
eye_diameter_max = \"75 mm\"
bore_diameter = \"20 mm\"
hub_diameter = \"30 mm\"
b3 = \"30 mm\"
cutwater_radius = \"111 mm\"
channels = 2
wrap = \"180 deg\"
";

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hmflow-cli"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hmflow-casing-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("carcasa.toml"), CASING).unwrap();
    dir
}

fn design(dir: &Path) -> Output {
    run(
        dir,
        &[
            "design",
            "--q",
            "120 m3/h",
            "--h",
            "45 m",
            "--n",
            "2900 rpm",
            "--casing",
            "carcasa.toml",
            "--out",
            "rodete.hmf",
        ],
    )
}

#[test]
fn design_check_report_and_recalc_work_end_to_end() {
    let dir = temp_dir("flujo");
    let designed = design(&dir);
    assert!(designed.status.success(), "{}", text(&designed));
    let printed = text(&designed);
    assert!(printed.contains("Carcasa"), "{printed}");
    assert!(!printed.contains("A360"), "{printed}");

    let file = std::fs::read_to_string(dir.join("rodete.hmf")).unwrap();
    assert!(file.contains("[casing]"), "{file}");
    assert!(file.contains("neck_diameter = \"90 mm\""), "{file}");

    let checked = run(&dir, &["check", "rodete.hmf"]);
    assert!(checked.status.success(), "{}", text(&checked));

    let reported = run(&dir, &["report", "rodete.hmf"]);
    assert!(reported.status.success(), "{}", text(&reported));
    let json = String::from_utf8_lossy(&reported.stdout);
    assert!(json.contains("\"casing\""), "{json}");

    let recalculated = run(&dir, &["recalc", "rodete.hmf"]);
    assert!(recalculated.status.success(), "{}", text(&recalculated));
    let after = std::fs::read_to_string(dir.join("rodete.hmf")).unwrap();
    assert_eq!(file, after);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_casing_file_names_its_path() {
    let dir = temp_dir("falta");
    std::fs::remove_file(dir.join("carcasa.toml")).unwrap();
    let output = design(&dir);
    assert!(!output.status.success());
    assert!(text(&output).contains("carcasa.toml"), "{}", text(&output));
    assert!(!dir.join("rodete.hmf").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_bad_casing_file_writes_nothing() {
    let dir = temp_dir("mala");
    let bad = CASING.replace("90 mm", "300 mm");
    std::fs::write(dir.join("carcasa.toml"), bad).unwrap();
    let output = design(&dir);
    assert!(!output.status.success());
    assert!(text(&output).contains("neck_diameter"), "{}", text(&output));
    assert!(!dir.join("rodete.hmf").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_volute_flag_that_contradicts_the_casing_is_an_error() {
    let dir = temp_dir("contradice");
    let output = run(
        &dir,
        &[
            "design",
            "--q",
            "120 m3/h",
            "--h",
            "45 m",
            "--n",
            "2900 rpm",
            "--casing",
            "carcasa.toml",
            "--volute-channels",
            "1",
            "--out",
            "rodete.hmf",
        ],
    );
    assert!(!output.status.success());
    assert!(text(&output).contains("[casing]"), "{}", text(&output));
    std::fs::remove_dir_all(&dir).ok();
}
