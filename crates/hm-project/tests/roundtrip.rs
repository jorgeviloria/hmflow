use hm_formula::Registry;
use hm_project::{Param, Project, PumpType};
use hm_units::parse_quantity;

#[test]
fn new_project_is_valid() {
    let project = Project::new("demo", PumpType::EndSuction);
    let diagnostics = project.validate(&Registry::builtin());
    assert!(!diagnostics.has_errors(), "{:?}", diagnostics.into_vec());
}

#[test]
fn roundtrip_toml() {
    let project = Project::new("demo", PumpType::SplitCase);
    let text = project.to_toml().unwrap();
    let parsed = Project::from_toml(&text).unwrap();

    assert_eq!(parsed.project.name, "demo");
    assert_eq!(parsed.design.pump_type, PumpType::SplitCase);
    assert_eq!(parsed.operating.q, "120 m3/h");
}

#[test]
fn toml_uses_expected_keys() {
    let project = Project::new("demo", PumpType::EndSuction);
    let text = project.to_toml().unwrap();
    assert!(text.contains("Q = "));
    assert!(text.contains("H = "));
    assert!(text.contains("pump_type = \"end_suction\""));
}

#[test]
fn invalid_flow_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.operating.q = "mucho".to_string();
    let diagnostics = project.validate(&Registry::builtin());
    assert!(diagnostics.has_errors());
}

#[test]
fn unknown_formula_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.params.insert(
        "d2".to_string(),
        Param::Auto {
            formula: "no.existe".to_string(),
        },
    );
    let diagnostics = project.validate(&Registry::builtin());
    assert!(diagnostics.has_errors());
}

#[test]
fn known_formula_is_accepted() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.params.insert(
        "H".to_string(),
        Param::Auto {
            formula: "impeller.head_euler".to_string(),
        },
    );
    let diagnostics = project.validate(&Registry::builtin());
    assert!(!diagnostics.has_errors());
}

#[test]
fn override_requires_author_date_and_reason() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.params.insert(
        "d2".to_string(),
        Param::Override {
            expr: "0.3 m".to_string(),
            author: String::new(),
            date: String::new(),
            reason: String::new(),
        },
    );
    let diagnostics = project.validate(&Registry::builtin());
    assert!(diagnostics.has_errors());
}

#[test]
fn pump_type_aliases() {
    assert_eq!(PumpType::parse("oh2"), Some(PumpType::EndSuction));
    assert_eq!(PumpType::parse("bb5"), Some(PumpType::Barrel));
    assert_eq!(PumpType::parse("inventada"), None);
}

#[test]
fn sample_quantities_parse() {
    let q = parse_quantity("120 m3/h").unwrap();
    assert!((q.value() - 120.0).abs() < 1e-9);
}
