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
    for model_volute in ["A360", "DN ", "difusor"] {
        assert!(!printed.contains(model_volute), "{model_volute}: {printed}");
    }
    assert!(printed.contains("dz/d2"), "{printed}");
    let z: u32 = printed
        .split("Z = ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|text| text.parse().ok())
        .unwrap();
    assert_eq!(z % 2, 1, "con dos canales, Z impar: {printed}");
    for warning in ["HM-DES-039", "HM-DES-040"] {
        assert!(printed.contains(warning), "{warning}: {printed}");
    }

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

const LEAKAGE: &str = "seal_clearance = \"0.3 mm\"
seal_length = \"15 mm\"
rear_seal_diameter = \"80 mm\"
rear_seal_length = \"15 mm\"
balance_hole_count = 6
balance_hole_diameter = \"9 mm\"
balance_hole_circle = \"50 mm\"
";

#[test]
fn the_leakage_data_goes_through_the_whole_flow() {
    let dir = temp_dir("fuga");
    std::fs::write(dir.join("carcasa.toml"), format!("{CASING}{LEAKAGE}")).unwrap();
    let designed = design(&dir);
    assert!(designed.status.success(), "{}", text(&designed));
    assert!(
        text(&designed).contains("Fugas por geometría"),
        "{}",
        text(&designed)
    );
    assert!(
        !text(&designed).contains("ηv anillo delantero"),
        "{}",
        text(&designed)
    );
    assert!(
        text(&designed).contains("Δd = 0.30 mm"),
        "{}",
        text(&designed)
    );

    let file = std::fs::read_to_string(dir.join("rodete.hmf")).unwrap();
    assert!(file.contains("rear_seal_diameter = \"80 mm\""), "{file}");

    let checked = run(&dir, &["check", "rodete.hmf"]);
    assert!(checked.status.success(), "{}", text(&checked));
    let reported = run(&dir, &["report", "rodete.hmf"]);
    assert!(String::from_utf8_lossy(&reported.stdout).contains("\"leakage\""));
    let recalculated = run(&dir, &["recalc", "rodete.hmf"]);
    assert!(recalculated.status.success(), "{}", text(&recalculated));
    assert!(
        !text(&recalculated).contains(" → "),
        "{}",
        text(&recalculated)
    );

    // ν depende de ρ, y `design` escribe ρ redondeada: la primera pasada mueve la
    // duodécima cifra. Después el archivo no cambia.
    let once = std::fs::read_to_string(dir.join("rodete.hmf")).unwrap();
    assert!(run(&dir, &["recalc", "rodete.hmf"]).status.success());
    assert_eq!(
        once,
        std::fs::read_to_string(dir.join("rodete.hmf")).unwrap()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_written_eta_v_does_not_pass_for_the_geometric_one() {
    let dir = temp_dir("eta-v");
    std::fs::write(dir.join("carcasa.toml"), format!("{CASING}{LEAKAGE}")).unwrap();
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
            "--eta-v",
            "0.93",
            "--out",
            "rodete.hmf",
        ],
    );
    let printed = text(&output);
    assert!(output.status.success(), "{printed}");
    assert!(printed.contains("ηv del método 0.9300"), "{printed}");
    assert!(!printed.contains("ηv por fugas 0.9300"), "{printed}");
    std::fs::remove_dir_all(&dir).ok();
}

fn design_with(dir: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
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
    ];
    args.extend_from_slice(extra);
    run(dir, &args)
}

fn section(file: &str, name: &str) -> String {
    let start = file.find(&format!("[params.{name}]")).unwrap();
    file[start..]
        .split("\n\n")
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn phi_and_blade_count_are_kept_as_decisions() {
    let dir = temp_dir("fixed");
    let done = design_with(&dir, &["--phi", "0.08", "--blade-count", "7"]);
    assert!(done.status.success(), "{}", text(&done));

    let saved = || std::fs::read_to_string(dir.join("rodete.hmf")).unwrap();
    let file = saved();
    for (name, value) in [("phi", "0.08"), ("blade_count", "7")] {
        let block = section(&file, name);
        assert!(block.contains("mode = \"locked\""), "{block}");
        assert!(block.contains(&format!("value = \"{value}\"")), "{block}");
    }

    let checked = run(&dir, &["check", "rodete.hmf"]);
    assert!(checked.status.success(), "{}", text(&checked));
    assert!(!text(&checked).contains("HM-DES-013"), "{}", text(&checked));

    let reselected = run(&dir, &["recalc", "--reselect", "rodete.hmf"]);
    assert!(reselected.status.success(), "{}", text(&reselected));
    let file = saved();
    assert!(section(&file, "phi").contains("value = \"0.08\""), "{file}");
    assert!(section(&file, "blade_count").contains("value = \"7\""));
}

#[test]
fn without_the_options_the_engine_keeps_choosing() {
    let dir = temp_dir("free-choice");
    let done = design_with(&dir, &[]);
    assert!(done.status.success(), "{}", text(&done));
    let file = std::fs::read_to_string(dir.join("rodete.hmf")).unwrap();
    for name in ["phi", "blade_count"] {
        assert!(section(&file, name).contains("mode = \"manual\""), "{file}");
    }
}

#[test]
fn only_what_was_given_is_fixed() {
    let dir = temp_dir("only-phi");
    let done = design_with(&dir, &["--phi", "0.08"]);
    assert!(done.status.success(), "{}", text(&done));
    let file = std::fs::read_to_string(dir.join("rodete.hmf")).unwrap();
    assert!(section(&file, "phi").contains("mode = \"locked\""));
    assert!(section(&file, "blade_count").contains("mode = \"manual\""));
}

#[test]
fn the_options_work_without_a_casing() {
    let dir = temp_dir("no-casing");
    let done = run(
        &dir,
        &[
            "design",
            "--q",
            "120 m3/h",
            "--h",
            "45 m",
            "--n",
            "2900 rpm",
            "--phi",
            "0.09",
            "--blade-count",
            "5",
            "--out",
            "libre.hmf",
        ],
    );
    assert!(done.status.success(), "{}", text(&done));
    let file = std::fs::read_to_string(dir.join("libre.hmf")).unwrap();
    assert!(section(&file, "phi").contains("mode = \"locked\""));
    assert!(section(&file, "blade_count").contains("value = \"5\""));
}

#[test]
fn a_bad_value_is_rejected_and_nothing_is_written() {
    let dir = temp_dir("bad-phi");
    for extra in [
        ["--phi", "-0.1"],
        ["--blade-count", "0"],
        ["--blade-count", "101"],
    ] {
        let done = design_with(&dir, &extra);
        assert!(!done.status.success(), "{}", text(&done));
        assert!(!dir.join("rodete.hmf").exists());
    }
}

#[test]
fn a_blade_count_the_file_cannot_hold_names_the_limit() {
    let dir = temp_dir("z-limit");
    let done = design_with(&dir, &["--blade-count", "101"]);
    assert!(text(&done).contains("entre 1 y 100"), "{}", text(&done));
}

#[test]
fn a_rare_blade_count_is_still_a_readable_project() {
    for z in ["16", "100"] {
        let dir = temp_dir(&format!("z-{z}"));
        let done = design_with(&dir, &["--blade-count", z]);
        assert!(done.status.success(), "{z}: {}", text(&done));
        let checked = run(&dir, &["check", "rodete.hmf"]);
        assert!(checked.status.success(), "{z}: {}", text(&checked));
    }
}

#[test]
fn a_channel_count_the_file_cannot_hold_is_not_written() {
    let dir = temp_dir("channels-limit");
    let done = run(
        &dir,
        &[
            "design",
            "--q",
            "120 m3/h",
            "--h",
            "45 m",
            "--n",
            "2900 rpm",
            "--volute-channels",
            "101",
            "--out",
            "libre.hmf",
        ],
    );
    assert!(!dir.join("libre.hmf").exists(), "{}", text(&done));
}

#[test]
fn the_report_carries_the_impeller_losses_only_with_a_casing() {
    let dir = temp_dir("losses");
    let done = design_with(&dir, &["--roughness", "0.032 mm"]);
    assert!(done.status.success(), "{}", text(&done));
    let reported = run(&dir, &["report", "rodete.hmf"]);
    let json = String::from_utf8_lossy(&reported.stdout);
    for field in [
        "\"impeller_losses\"",
        "\"friction_smooth\"",
        "\"friction_rough\"",
        "\"roughness_eta_h\"",
        "\"shock\"",
    ] {
        assert!(json.contains(field), "{field}");
    }

    let free = run(
        &dir,
        &[
            "design",
            "--q",
            "120 m3/h",
            "--h",
            "45 m",
            "--n",
            "2900 rpm",
            "--out",
            "libre.hmf",
        ],
    );
    assert!(free.status.success(), "{}", text(&free));
    let reported = run(&dir, &["report", "libre.hmf"]);
    let json = String::from_utf8_lossy(&reported.stdout);
    assert!(!json.contains("impeller_losses"));
}
