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
fn wrong_dimensions_are_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.operating.q = "120 s".to_string();
    project.operating.h = "45 kg".to_string();
    project.operating.n = "2900 m".to_string();

    let diagnostics = project.validate(&Registry::builtin());
    assert!(diagnostics.has_errors());
    assert_eq!(diagnostics.into_vec().len(), 3);
}

#[test]
fn negative_flow_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.operating.q = "-120 m3/h".to_string();

    let diagnostics = project.validate(&Registry::builtin());
    assert!(diagnostics.has_errors());
}

#[test]
fn zero_stages_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.stages = 0;

    let diagnostics = project.validate(&Registry::builtin());
    assert!(diagnostics.has_errors());
}

#[test]
fn unknown_fields_are_rejected() {
    let text = r#"
schema = 1

[project]
name = "demo"

[design]
pump_type = "end_suction"
stages = 1

[operating]
Q = "120 m3/h"
H = "45 m"
n = "2900 rpm"
flow_rate = "120 m3/h"
"#;
    assert!(Project::from_toml(text).is_err());

    let text = r#"
schema = 1
flow_rate = "120 m3/h"

[project]
name = "demo"

[design]
pump_type = "end_suction"
stages = 1

[operating]
Q = "120 m3/h"
H = "45 m"
n = "2900 rpm"
"#;
    assert!(Project::from_toml(text).is_err());
}

#[test]
fn default_npsha_clears_the_sample_curve() {
    assert_eq!(
        Project::new("demo", PumpType::EndSuction).operating.npsha,
        "10 m"
    );
}

#[test]
fn default_standard_depends_on_type() {
    assert_eq!(
        Project::new("demo", PumpType::EndSuction).design.standard,
        "ISO 2858"
    );
    assert!(
        Project::new("demo", PumpType::Barrel)
            .design
            .standard
            .is_empty()
    );
}

#[test]
fn unknown_formula_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.params.insert(
        "d2".to_string(),
        Param::Auto {
            formula: "no.existe".to_string(),
            value: None,
        },
    );
    let diagnostics = project.validate(&Registry::builtin());
    assert!(diagnostics.has_errors());
}

#[test]
fn auto_value_with_wrong_unit_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.params.insert(
        "d2".to_string(),
        Param::Auto {
            formula: "design.d2".to_string(),
            value: Some("1 s".to_string()),
        },
    );
    let diagnostics = project.validate(&Registry::all());
    assert!(diagnostics.has_errors());
}

#[test]
fn known_formula_is_accepted() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.params.insert(
        "H".to_string(),
        Param::Auto {
            formula: "impeller.head_euler".to_string(),
            value: None,
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
