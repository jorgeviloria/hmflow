//! Salidas congeladas de la CLI.
//!
//! Cada caso ejecuta una o más órdenes en un directorio temporal, con los
//! fixtures copiados dentro y rutas relativas, y compara `stdout`, `stderr`,
//! el código de salida y los archivos escritos con `tests/golden/<caso>.txt`.
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

struct CommandSpec {
    args: &'static [&'static str],
    status: i32,
    /// Archivos que la orden debe escribir; se copian al golden.
    files: &'static [&'static str],
    /// Archivos que la orden no debe escribir.
    absent: &'static [&'static str],
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

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "b1",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design", "--name", "b1", "--q", "1060 gpm", "--h", "83 m", "--n", "1750 rpm",
                    "--npsha", "10 m",
                ],
                status: 0,
                files: &["b1.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "b1.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "d1a1",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
                    "d1a1",
                    "--q",
                    "0.016 m3/s",
                    "--h",
                    "13.8 m",
                    "--n",
                    "1800 rpm",
                ],
                status: 0,
                files: &["d1a1.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "d1a1.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
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
                files: &["ref.hmf", "curve.csv"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "ref.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "sweep-low",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
                    "sweep-low",
                    "--q",
                    "40 m3/h",
                    "--h",
                    "100 m",
                    "--n",
                    "2900 rpm",
                ],
                status: 0,
                files: &["sweep-low.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "sweep-low.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "sweep-mid",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
                    "sweep-mid",
                    "--q",
                    "120 m3/h",
                    "--h",
                    "45 m",
                    "--n",
                    "2900 rpm",
                    "--viscosity",
                    "40 cSt",
                ],
                status: 0,
                files: &["sweep-mid.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "sweep-mid.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "sweep-high",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
                    "sweep-high",
                    "--q",
                    "800 m3/h",
                    "--h",
                    "20 m",
                    "--n",
                    "1450 rpm",
                ],
                status: 0,
                files: &["sweep-high.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "sweep-high.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "double-volute",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
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
                ],
                status: 0,
                files: &["double-volute.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "double-volute.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "suction-water",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
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
                ],
                status: 0,
                files: &["suction-water.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "suction-water.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "suction-custom",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
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
                ],
                status: 0,
                files: &["suction-custom.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "suction-custom.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "hot-water",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
                    "hot-water",
                    "--q",
                    "120 m3/h",
                    "--h",
                    "45 m",
                    "--n",
                    "2900 rpm",
                    "--temperature",
                    "80 degC",
                ],
                status: 0,
                files: &["hot-water.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "hot-water.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "hot-water-rho",
        fixtures: NONE_FIXTURES,
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &[
                    "design",
                    "--name",
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
                ],
                status: 0,
                files: &["hot-water-rho.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "hot-water-rho.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "recalc-v0.8",
        fixtures: &[("end_suction_v0.8.hmf", "v0.8.hmf")],
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &["recalc", "v0.8.hmf"],
                status: 0,
                files: &["v0.8.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "v0.8.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "recalc-v0.2",
        fixtures: &[("end_suction_v0.2.hmf", "v0.2.hmf")],
        files: NONE_FILES,
        commands: &[
            CommandSpec {
                args: &["recalc", "v0.2.hmf"],
                status: 0,
                files: &["v0.2.hmf"],
                absent: NONE,
            },
            CommandSpec {
                args: &["report", "v0.2.hmf", "--curve-points", "11"],
                status: 0,
                files: NONE,
                absent: NONE,
            },
        ],
    },
    Scenario {
        name: "check-ref",
        fixtures: &[("end_suction_v0.9.hmf", "reference.hmf")],
        files: NONE_FILES,
        commands: &[CommandSpec {
            args: &["check", "reference.hmf"],
            status: 0,
            files: NONE,
            absent: NONE,
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
            absent: NONE,
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
            absent: NONE,
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
            absent: NONE,
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
            absent: &["invalid.hmf"],
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
            absent: NONE,
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
                absent: NONE,
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
                absent: &["curve.hmf"],
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
            absent: &["invalid-diagnostic.hmf"],
        }],
    },
];

const NONE_FIXTURES: &[(&str, &str)] = &[];
const NONE_FILES: &[(&str, &str)] = &[];

#[test]
fn golden_outputs_do_not_change() {
    let bless = std::env::var_os("HMFLOW_BLESS").is_some();
    if bless && std::env::var_os("CI").is_some() {
        panic!("HMFLOW_BLESS no se permite en CI; revisa el cambio y bendice en local");
    }

    let golden_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    fs::create_dir_all(&golden_dir).expect("no se pudo crear tests/golden");

    for scenario in SCENARIOS {
        let actual = run_scenario(scenario);
        let path = golden_dir.join(format!("{}.txt", scenario.name));

        if bless {
            fs::write(&path, &actual)
                .unwrap_or_else(|error| panic!("no se pudo escribir {}: {error}", path.display()));
            continue;
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
            "la salida de `{}` cambió.\n\
             Si es intencional: HMFLOW_BLESS=1 cargo test -p hmflow-cli --test golden\n\
             y revisa el diff de tests/golden/.",
            scenario.name
        );
    }
}

fn run_scenario(scenario: &Scenario) -> String {
    let directory = temp_directory(scenario.name);
    prepare(scenario, &directory);

    let mut record = String::new();
    for command in scenario.commands {
        let output = Command::new(cli())
            .args(command.args.iter().copied())
            .current_dir(&directory)
            .output()
            .unwrap_or_else(|error| {
                panic!("no se pudo ejecutar `{}`: {error}", command.args.join(" "))
            });

        let status = output.status.code().unwrap_or(-1);
        assert_eq!(
            status,
            command.status,
            "caso `{}`: `hmflow-cli {}` terminó con {status}, se esperaba {}.\nstdout:\n{}\nstderr:\n{}",
            scenario.name,
            command.args.join(" "),
            command.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        record.push_str("# command\n$ hmflow-cli ");
        record.push_str(&command.args.join(" "));
        record.push_str("\n# stdout\n");
        record.push_str(&String::from_utf8_lossy(&output.stdout));
        if !record.ends_with('\n') {
            record.push('\n');
        }
        record.push_str("# stderr\n");
        record.push_str(&String::from_utf8_lossy(&output.stderr));
        if !record.ends_with('\n') {
            record.push('\n');
        }
        record.push_str("# status\n");
        record.push_str(&status.to_string());
        record.push('\n');

        for file in command.files {
            let path = directory.join(file);
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

        for file in command.absent {
            let path = directory.join(file);
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

    let normalized = normalize(&record, &directory);
    let _ = fs::remove_dir_all(&directory);
    normalized
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

            if has_dot && fraction_digits > 9 {
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
