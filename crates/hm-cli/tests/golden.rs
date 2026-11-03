//! Salidas congeladas de la CLI.
//!
//! Cada escenario es un test. Ejecuta sus órdenes en un directorio temporal,
//! con los fixtures copiados dentro y rutas relativas, y compara el texto que
//! vería quien las ejecuta con `tests/golden/<caso>.txt`.
//!
//! Se guarda el stdout de `design`, `check` y `recalc`, el stderr, el código
//! de salida y los archivos que la orden no debe escribir. El `.hmf` se
//! comprueba que exista, sin copiarlo: el proyecto ya tiene pruebas propias.
//! El informe JSON se guarda solo en `ref`; en el resto se exige el formato
//! `hmflow.design-report` y no se archiva, para que un campo nuevo no
//! reescriba todos los casos.
//!
//! Para regenerar las salidas:
//!
//! ```text
//! HMFLOW_BLESS=1 cargo test -p hmflow-cli --test golden
//! ```
//!
//! En CI no se bendice nunca: si `CI` está definido, `HMFLOW_BLESS` falla.
//!
//! Los flotantes con más de 9 decimales se redondean a 9 antes de comparar:
//! la última cifra de `libm` puede cambiar entre Linux, macOS y Windows, y la
//! golden no debe romperse por una diferencia de 1 ulp. Las pruebas numéricas
//! del proyecto conservan la precisión completa.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const NONE: &[&str] = &[];
const NONE_FIXTURES: &[(&str, &str)] = &[];
const NONE_FILES: &[(&str, &str)] = &[];

/// Qué parte de la orden entra en el archivo golden.
#[derive(Clone, Copy)]
enum Capture {
    /// Stdout, stderr y estado.
    Transcript,
    /// El stdout es el informe JSON: se comprueba el formato y no se guarda.
    ReportStatus,
}

struct CommandSpec {
    args: &'static [&'static str],
    status: i32,
    /// Archivos cuyo contenido se copia al golden.
    files: &'static [&'static str],
    /// Archivos que la orden debe escribir y cuyo contenido no se copia.
    present: &'static [&'static str],
    /// Archivos que la orden no debe escribir.
    absent: &'static [&'static str],
    capture: Capture,
}

struct Scenario {
    name: &'static str,
    /// Fixtures del repositorio que se copian al directorio temporal:
    /// (nombre en `hm-project/tests/fixtures`, nombre local).
    fixtures: &'static [(&'static str, &'static str)],
    /// Archivos sintéticos que se escriben en el directorio temporal.
    files: &'static [(&'static str, &'static str)],
    commands: &'static [CommandSpec],
}

const INLINE_HMF: &str = r#"schema = 2

[project]
name = "inline"

[design]
pump_type = "inline"
stages = 1

[operating]
Q = "120 m3/h"
H = "45 m"
n = "2900 rpm"
"#;

const OVERRIDE_HMF: &str = r#"schema = 2

[project]
name = "override"

[design]
pump_type = "end_suction"
stages = 1
standard = "ISO 2858"
method = "end_suction_v0.9"

[operating]
Q = "120 m3/h"
H = "45 m"
n = "2900 rpm"
fluid = "water"
temperature = "20 degC"
npsha = "15 m"

[params.psi]
mode = "override"
expr = "0.5"
author = "test"
date = "2026-11-03"
reason = "golden"
"#;

/// `design` seguido de `report`. El informe no se archiva.
macro_rules! design_case {
    ($name:literal, $($arg:literal),+ $(,)?) => {
        Scenario {
            name: $name,
            fixtures: NONE_FIXTURES,
            files: NONE_FILES,
            commands: &[
                CommandSpec {
                    args: &["design", "--name", $name, $($arg),+],
                    status: 0,
                    files: NONE,
                    present: &[concat!($name, ".hmf")],
                    absent: NONE,
                    capture: Capture::Transcript,
                },
                CommandSpec {
                    args: &["report", concat!($name, ".hmf"), "--curve-points", "11"],
                    status: 0,
                    files: NONE,
                    present: NONE,
                    absent: NONE,
                    capture: Capture::ReportStatus,
                },
            ],
        }
    };
}

/// `recalc` de un fixture seguido de `report`. El `.hmf` migrado no se archiva.
macro_rules! recalc_case {
    ($name:literal, $fixture:literal, $local:literal) => {
        Scenario {
            name: $name,
            fixtures: &[($fixture, $local)],
            files: NONE_FILES,
            commands: &[
                CommandSpec {
                    args: &["recalc", $local],
                    status: 0,
                    files: NONE,
                    present: &[$local],
                    absent: NONE,
                    capture: Capture::Transcript,
                },
                CommandSpec {
                    args: &["report", $local, "--curve-points", "11"],
                    status: 0,
                    files: NONE,
                    present: NONE,
                    absent: NONE,
                    capture: Capture::ReportStatus,
                },
            ],
        }
    };
}

const SCENARIOS: &[Scenario] = &[
    design_case!(
        "b1", "--q", "1060 gpm", "--h", "83 m", "--n", "1750 rpm", "--npsha", "10 m",
    ),
    design_case!(
        "d1a1",
        "--q",
        "0.016 m3/s",
        "--h",
        "13.8 m",
        "--n",
        "1800 rpm",
    ),
    Scenario {
        name: "ref",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
                    "ref",
                    "--q",
                    "120 m3/h",
                    "--h",
                    "45 m",
                    "--n",
                    "2900 rpm",
                    "--npsha",
                    "15 m",
                    "--curves",
                    "--curves-out",
                    "curve.csv",
                ],
                status: 0,
                files: &["curve.csv"],
                present: &["ref.hmf"],
                absent: NONE,
                capture: Capture::Transcript,
            },
            CommandSpec {
                args: &["report", "ref.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                present: NONE,
                absent: NONE,
                capture: Capture::Transcript,
            },
        ],
    },
    design_case!(
        "sweep-low",
        "--q",
        "40 m3/h",
        "--h",
        "100 m",
        "--n",
        "2900 rpm"
    ),
    design_case!(
        "sweep-mid",
        "--q",
        "120 m3/h",
        "--h",
        "45 m",
        "--n",
        "2900 rpm",
        "--viscosity",
        "40 cSt",
    ),
    design_case!(
        "sweep-high",
        "--q",
        "800 m3/h",
        "--h",
        "20 m",
        "--n",
        "1450 rpm",
    ),
    design_case!(
        "double-volute",
        "--q",
        "120 m3/h",
        "--h",
        "45 m",
        "--n",
        "2900 rpm",
        "--npsha",
        "15 m",
        "--volute-channels",
        "2",
        "--volute-wrap",
        "165",
    ),
    design_case!(
        "suction-water",
        "--q",
        "120 m3/h",
        "--h",
        "45 m",
        "--n",
        "2900 rpm",
        "--suction-pressure",
        "1 atm",
        "--suction-height",
        "2 m",
        "--suction-losses",
        "0.5 m",
    ),
    design_case!(
        "suction-custom",
        "--q",
        "120 m3/h",
        "--h",
        "45 m",
        "--n",
        "2900 rpm",
        "--fluid",
        "custom",
        "--rho",
        "850 kg/m3",
        "--suction-pressure",
        "1 atm",
        "--suction-height",
        "2 m",
        "--suction-losses",
        "0.5 m",
        "--vapor-pressure",
        "5 kPa",
    ),
    design_case!(
        "hot-water",
        "--q",
        "120 m3/h",
        "--h",
        "45 m",
        "--n",
        "2900 rpm",
        "--temperature",
        "80 degC",
    ),
    design_case!(
        "hot-water-rho",
        "--q",
        "120 m3/h",
        "--h",
        "45 m",
        "--n",
        "2900 rpm",
        "--temperature",
        "80 degC",
        "--rho",
        "998.2 kg/m3",
    ),
    recalc_case!("recalc-v0.8", "end_suction_v0.8.hmf", "v0.8.hmf"),
    recalc_case!("recalc-v0.2", "end_suction_v0.2.hmf", "v0.2.hmf"),
    Scenario {
        name: "check-ref",
        fixtures: &[("end_suction_v0.9.hmf", "reference.hmf")],
        files: NONE_FILES,
        commands: &[CommandSpec {
            args: &["check", "reference.hmf"],
            status: 0,
            files: NONE,
            present: NONE,
            absent: NONE,
            capture: Capture::Transcript,
        }],
    },
    Scenario {
        name: "check-v0.8",
        fixtures: &[("end_suction_v0.8.hmf", "v0.8.hmf")],
        files: NONE_FILES,
        commands: &[CommandSpec {
            args: &["check", "v0.8.hmf"],
            status: 1,
            files: NONE,
            present: NONE,
            absent: NONE,
            capture: Capture::Transcript,
        }],
    },
    Scenario {
        name: "report-v0.8",
        fixtures: &[("end_suction_v0.8.hmf", "v0.8.hmf")],
        files: NONE_FILES,
        commands: &[CommandSpec {
            args: &["report", "v0.8.hmf"],
            status: 1,
            files: NONE,
            present: NONE,
            absent: NONE,
            capture: Capture::Transcript,
        }],
    },
    Scenario {
        name: "recalc-inline",
        fixtures: NONE_FIXTURES,
        files: &[("inline.hmf", INLINE_HMF)],
        commands: &[CommandSpec {
            args: &["recalc", "inline.hmf"],
            status: 1,
            files: NONE,
            present: NONE,
            absent: NONE,
            capture: Capture::Transcript,
        }],
    },
    Scenario {
        name: "design-negative-q",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[CommandSpec {
            args: &[
                "design", "--name", "invalid", "--q", "-1 m3/h", "--h", "45 m", "--n", "2900 rpm",
            ],
            status: 1,
            files: NONE,
            present: NONE,
            absent: &["invalid.hmf"],
            capture: Capture::Transcript,
        }],
    },
    Scenario {
        name: "report-override",
        fixtures: NONE_FIXTURES,
        files: &[("override.hmf", OVERRIDE_HMF)],
        commands: &[CommandSpec {
            args: &["report", "override.hmf"],
            status: 1,
            files: NONE,
            present: NONE,
            absent: NONE,
            capture: Capture::Transcript,
        }],
    },
    Scenario {
        name: "curve-points",
        fixtures: &[("end_suction_v0.9.hmf", "ref.hmf")],
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &["report", "ref.hmf", "--curve-points", "1"],
                status: 1,
                files: NONE,
                present: NONE,
                absent: NONE,
                capture: Capture::Transcript,
            },
            CommandSpec {
                args: &[
                    "design",
                    "--name",
                    "curve",
                    "--q",
                    "120 m3/h",
                    "--h",
                    "45 m",
                    "--n",
                    "2900 rpm",
                    "--curve-points",
                    "1",
                ],
                status: 1,
                files: NONE,
                present: NONE,
                absent: &["curve.hmf"],
                capture: Capture::Transcript,
            },
        ],
    },
    Scenario {
        name: "design-with-errors",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[CommandSpec {
            args: &[
                "design",
                "--name",
                "invalid-diagnostic",
                "--q",
                "120 m3/h",
                "--h",
                "45 m",
                "--n",
                "2900 rpm",
                "--npsha",
                "0.1 m",
            ],
            status: 1,
            files: NONE,
            present: NONE,
            absent: &["invalid-diagnostic.hmf"],
            capture: Capture::Transcript,
        }],
    },
];

macro_rules! cases {
    ($($fn_name:ident = $name:literal),* $(,)?) => {
        const CASE_NAMES: &[&str] = &[$($name),*];

        $(
            #[test]
            fn $fn_name() {
                check($name);
            }
        )*
    };
}

cases! {
    b1 = "b1",
    d1a1 = "d1a1",
    ref_case = "ref",
    sweep_low = "sweep-low",
    sweep_mid = "sweep-mid",
    sweep_high = "sweep-high",
    double_volute = "double-volute",
    suction_water = "suction-water",
    suction_custom = "suction-custom",
    hot_water = "hot-water",
    hot_water_rho = "hot-water-rho",
    recalc_v0_8 = "recalc-v0.8",
    recalc_v0_2 = "recalc-v0.2",
    check_ref = "check-ref",
    check_v0_8 = "check-v0.8",
    report_v0_8 = "report-v0.8",
    recalc_inline = "recalc-inline",
    design_negative_q = "design-negative-q",
    report_override = "report-override",
    curve_points = "curve-points",
    design_with_errors = "design-with-errors",
}

#[test]
fn every_scenario_is_its_own_test() {
    let mut from_table: Vec<_> = SCENARIOS.iter().map(|scenario| scenario.name).collect();
    let mut from_tests = CASE_NAMES.to_vec();
    from_table.sort_unstable();
    from_tests.sort_unstable();
    assert_eq!(
        from_table, from_tests,
        "cada escenario de SCENARIOS tiene que tener su test en cases!"
    );

    // En modo bendición los tests escriben en paralelo; no se exige que el
    // directorio esté completo hasta la siguiente ejecución normal.
    if std::env::var_os("HMFLOW_BLESS").is_some() {
        return;
    }

    let golden_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut from_disk: Vec<String> = fs::read_dir(&golden_dir)
        .unwrap_or_else(|error| panic!("no se pudo leer {}: {error}", golden_dir.display()))
        .map(|entry| entry.expect("entrada de tests/golden").path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("txt"))
        .filter_map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })
        .collect();
    from_disk.sort_unstable();

    let mut from_tests: Vec<String> = CASE_NAMES.iter().map(|name| (*name).to_string()).collect();
    from_tests.sort_unstable();
    assert_eq!(
        from_disk, from_tests,
        "tests/golden/ tiene archivos de más o de menos respecto a cases!"
    );
}

fn check(name: &str) {
    let bless = std::env::var_os("HMFLOW_BLESS").is_some();
    if bless && std::env::var_os("CI").is_some() {
        panic!("HMFLOW_BLESS no se permite en CI; revisa el cambio y bendice en local");
    }

    let scenario = SCENARIOS
        .iter()
        .find(|scenario| scenario.name == name)
        .unwrap_or_else(|| panic!("no existe el escenario `{name}`"));
    let actual = run_scenario(scenario);
    let golden_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    fs::create_dir_all(&golden_dir).expect("no se pudo crear tests/golden");
    let path = golden_dir.join(format!("{name}.txt"));

    if bless {
        fs::write(&path, &actual)
            .unwrap_or_else(|error| panic!("no se pudo escribir {}: {error}", path.display()));
        return;
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "no se pudo leer {}: {error}\n\
             genera las salidas con HMFLOW_BLESS=1 cargo test -p hmflow-cli --test golden",
            path.display()
        )
    });

    assert_eq!(
        actual, expected,
        "la salida de `{name}` cambió.\n\
         Si es intencional: HMFLOW_BLESS=1 cargo test -p hmflow-cli --test golden\n\
         y revisa el diff de tests/golden/."
    );
}

fn run_scenario(scenario: &Scenario) -> String {
    let directory = TempDirectory::new(scenario.name);
    prepare(scenario, directory.path());

    let mut record = String::new();
    for command in scenario.commands {
        let output = Command::new(cli())
            .args(command.args.iter().copied())
            .current_dir(directory.path())
            .output()
            .unwrap_or_else(|error| {
                panic!("no se pudo ejecutar `{}`: {error}", command.args.join(" "))
            });

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status = output.status.code().unwrap_or(-1);
        assert_eq!(
            status,
            command.status,
            "caso `{}`: `hmflow-cli {}` terminó con {status}, se esperaba {}.\nstdout:\n{stdout}\nstderr:\n{stderr}",
            scenario.name,
            command.args.join(" "),
            command.status,
        );

        if matches!(command.capture, Capture::ReportStatus) {
            assert_report_json(scenario.name, &command.args.join(" "), &stdout);
        }

        record.push_str("# command\n$ hmflow-cli ");
        record.push_str(&command.args.join(" "));
        record.push('\n');
        if matches!(command.capture, Capture::Transcript) {
            record.push_str("# stdout\n");
            record.push_str(&stdout);
            if !record.ends_with('\n') {
                record.push('\n');
            }
        }
        record.push_str("# stderr\n");
        record.push_str(&stderr);
        if !record.ends_with('\n') {
            record.push('\n');
        }
        record.push_str("# status\n");
        record.push_str(&status.to_string());
        record.push('\n');

        for file in command.files {
            let path = directory.path().join(file);
            let contents = fs::read(&path).unwrap_or_else(|error| {
                panic!(
                    "caso `{}`: no se pudo leer {} después de `{}`: {error}",
                    scenario.name,
                    path.display(),
                    command.args.join(" ")
                )
            });
            record.push_str(&format!("# file {file}\n"));
            record.push_str(&String::from_utf8_lossy(&contents));
            if !record.ends_with('\n') {
                record.push('\n');
            }
        }

        for file in command.present {
            let path = directory.path().join(file);
            assert!(
                path.is_file(),
                "caso `{}`: `{}` debería existir después de `{}`",
                scenario.name,
                path.display(),
                command.args.join(" ")
            );
            record.push_str(&format!("# present {file}\n"));
        }

        for file in command.absent {
            let path = directory.path().join(file);
            assert!(
                !path.exists(),
                "caso `{}`: `{}` no debería existir después de `{}`",
                scenario.name,
                path.display(),
                command.args.join(" ")
            );
            record.push_str(&format!("# absent {file}\n"));
        }
    }

    normalize(&record, directory.path())
}

fn assert_report_json(scenario: &str, args: &str, stdout: &str) {
    let sample: String = stdout.chars().take(240).collect();
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
        panic!("caso `{scenario}`: `hmflow-cli {args}` no escribió JSON válido: {error}\n{sample}")
    });
    let format = value.get("format").and_then(|value| value.as_str());
    assert_eq!(
        format,
        Some(hm_design::REPORT_FORMAT),
        "caso `{scenario}`: `hmflow-cli {args}` no escribió el informe de hmflow.\n{sample}"
    );
}

fn prepare(scenario: &Scenario, directory: &Path) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../hm-project/tests/fixtures");

    for (from, to) in scenario.fixtures {
        let source = fixtures.join(from);
        let destination = directory.join(to);
        fs::copy(&source, &destination).unwrap_or_else(|error| {
            panic!(
                "no se pudo copiar {} a {}: {error}",
                source.display(),
                destination.display()
            )
        });
    }

    for (name, contents) in scenario.files {
        fs::write(directory.join(name), contents)
            .unwrap_or_else(|error| panic!("no se pudo escribir {name}: {error}"));
    }
}

fn normalize(text: &str, directory: &Path) -> String {
    let mut text = text.replace('\r', "");

    let directory = directory.to_string_lossy();
    let directory_slash = directory.replace('\\', "/");
    text = text.replace(directory.as_ref(), "<dir>");
    text = text.replace(&directory_slash, "<dir>");

    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    if let Some(repository) = manifest.parent() {
        let repository = repository.to_string_lossy();
        let repository_slash = repository.replace('\\', "/");
        text = text.replace(repository.as_ref(), "<repo>");
        text = text.replace(&repository_slash, "<repo>");
    }

    normalize_floats(&text.replace('\\', "/"))
}

/// Redondea los flotantes largos para que la golden no dependa de la última
/// cifra de `libm` en cada plataforma.
fn normalize_floats(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;

    while index < chars.len() {
        let start = index;
        let signed = matches!(chars[index], '-' | '+')
            && index + 1 < chars.len()
            && chars[index + 1].is_ascii_digit();

        if chars[index].is_ascii_digit() || signed {
            let mut end = index;
            if signed {
                end += 1;
            }
            while end < chars.len() && chars[end].is_ascii_digit() {
                end += 1;
            }

            let mut has_dot = false;
            let mut fraction_digits = 0;
            if end < chars.len() && chars[end] == '.' {
                has_dot = true;
                end += 1;
                while end < chars.len() && chars[end].is_ascii_digit() {
                    end += 1;
                    fraction_digits += 1;
                }
            }

            let mut has_exponent = false;
            if end < chars.len() && matches!(chars[end], 'e' | 'E') {
                let mut exponent = end + 1;
                if exponent < chars.len() && matches!(chars[exponent], '+' | '-') {
                    exponent += 1;
                }
                let exponent_start = exponent;
                while exponent < chars.len() && chars[exponent].is_ascii_digit() {
                    exponent += 1;
                }
                if exponent > exponent_start {
                    has_exponent = true;
                    end = exponent;
                }
            }

            if has_exponent || (has_dot && fraction_digits > 9) {
                let token: String = chars[start..end].iter().collect();
                if let Ok(value) = token.parse::<f64>() {
                    if has_exponent {
                        out.push_str(&format!("{value:.9e}"));
                    } else {
                        out.push_str(&format!("{value:.9}"));
                    }
                    index = end;
                    continue;
                }
            }

            out.extend(chars[start..end].iter());
            index = end;
            continue;
        }

        out.push(chars[index]);
        index += 1;
    }

    out
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(name: &str) -> Self {
        Self(temp_directory(name))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0)
            && self.0.exists()
        {
            eprintln!("no se pudo limpiar {}: {error}", self.0.display());
        }
    }
}

fn temp_directory(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "hmflow-golden-{}-{}-{}",
        std::process::id(),
        unique,
        name
    ));

    if directory.exists() {
        fs::remove_dir_all(&directory)
            .unwrap_or_else(|error| panic!("no se pudo limpiar {}: {error}", directory.display()));
    }
    fs::create_dir_all(&directory)
        .unwrap_or_else(|error| panic!("no se pudo crear {}: {error}", directory.display()));
    directory
}

fn cli() -> &'static str {
    env!("CARGO_BIN_EXE_hmflow-cli")
}

#[test]
fn long_floats_are_normalized() {
    assert_eq!(normalize_floats("0.123456789123"), "0.123456789");
    assert_eq!(normalize_floats("1e-7"), "1.000000000e-7");
    assert_eq!(normalize_floats("1.234567890123e-7"), "1.234567890e-7");
    assert_eq!(normalize_floats("83.0"), "83.0");
}
