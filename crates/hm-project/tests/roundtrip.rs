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
fn unknown_param_field_is_rejected() {
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

[params.d2]
mode = "manual"
value = "0.2 m"
extra = "x"
"#;
    assert!(Project::from_toml(text).is_err());
}

#[test]
fn non_finite_quantity_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.operating.q = "1e309 m3/h".to_string();

    let diagnostics = project.validate(&Registry::builtin());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-PROJ-002")
    );
}

#[test]
fn two_stage_end_suction_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.stages = 2;

    let diagnostics = project.validate(&Registry::builtin());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-PROJ-024")
    );
}

#[test]
fn iso_2858_for_barrel_is_reported() {
    let mut project = Project::new("demo", PumpType::Barrel);
    project.design.standard = "ISO 2858".to_string();

    let diagnostics = project.validate(&Registry::builtin());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-PROJ-022")
    );
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
fn new_project_does_not_invent_npsha() {
    assert!(
        Project::new("demo", PumpType::EndSuction)
            .operating
            .npsha
            .is_empty()
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

/// Un `.hmf` de esquema 1 escrito por `design` con el método
/// end_suction_v0.2. No se edita: es la prueba de la migración 1 → 2.
#[test]
fn schema_1_fixture_migrates_and_validates() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/end_suction_v0.2.hmf");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.starts_with("schema = 1\n"),
        "el archivo debe seguir en esquema 1"
    );
    let project = Project::load(&path).unwrap();
    assert_eq!(project.schema, hm_project::SCHEMA_VERSION);
    assert!(project.suction.is_none());
    assert!(project.operating.viscosity.is_empty());
    assert_eq!(project.design.method, "end_suction_v0.2");
    let diagnostics = project.validate(&Registry::all());
    assert!(!diagnostics.has_errors());
}

fn codes(project: &Project) -> Vec<String> {
    project
        .validate(&Registry::all())
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect()
}

fn with_suction(pressure: &str, height: &str, losses: &str, vapor: &str) -> Project {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.suction = Some(hm_project::Suction {
        pressure: pressure.to_string(),
        height: height.to_string(),
        losses: losses.to_string(),
        vapor_pressure: vapor.to_string(),
    });
    project
}

#[test]
fn unknown_schema_is_not_migrated() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.schema = 6;
    let text = project.to_toml().unwrap();
    let project = Project::from_toml(&text).unwrap();
    assert_eq!(project.schema, 6);
    assert_eq!(codes(&project), ["HM-PROJ-001"]);
}

#[test]
fn suction_and_viscosity_round_trip() {
    let mut project = with_suction("1 atm", "-2 m", "0.5 m", "");
    project.operating.viscosity = "30 cSt".to_string();
    let text = project.to_toml().unwrap();
    assert!(text.contains("[suction]"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    let suction = back.suction.unwrap();
    assert_eq!(suction.pressure, "1 atm");
    assert_eq!(suction.height, "-2 m");
    assert_eq!(back.operating.viscosity, "30 cSt");
}

#[test]
fn valid_suction_and_viscosity_pass() {
    let mut project = with_suction("101325 Pa", "-3 m", "", "");
    project.operating.viscosity = "1.2 cP".to_string();
    assert!(codes(&project).is_empty(), "{:?}", codes(&project));
}

#[test]
fn bad_suction_fields_are_reported() {
    for project in [
        with_suction("0 Pa", "", "", ""),
        with_suction("1 atm", "2 kg", "", ""),
        with_suction("1 atm", "", "-1 m", ""),
        with_suction("1 atm", "", "", "-5 Pa"),
        with_suction("una atmósfera", "", "", ""),
    ] {
        assert_eq!(codes(&project), ["HM-PROJ-027"]);
    }
}

#[test]
fn npsha_and_suction_at_once_is_reported() {
    let mut project = with_suction("1 atm", "", "", "");
    project.operating.npsha = "8 m".to_string();
    assert_eq!(codes(&project), ["HM-PROJ-028"]);
}

#[test]
fn other_fluids_need_a_vapor_pressure() {
    let mut project = with_suction("1 atm", "", "", "");
    project.operating.fluid = "glycol".to_string();
    assert_eq!(codes(&project), ["HM-PROJ-029"]);
    project.suction.as_mut().unwrap().vapor_pressure = "1.5 kPa".to_string();
    assert!(codes(&project).is_empty());
}

#[test]
fn bad_viscosity_is_reported() {
    for text in ["30 m", "-1 cSt", "mucha"] {
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.operating.viscosity = text.to_string();
        assert_eq!(codes(&project), ["HM-PROJ-026"], "{text}");
    }
}

#[test]
fn unknown_suction_fields_are_rejected() {
    let text = r#"
schema = 2

[project]
name = "demo"

[design]
pump_type = "end_suction"

[operating]
Q = "120 m3/h"
H = "45 m"
n = "2900 rpm"

[suction]
pressure = "1 atm"
depth = "3 m"
"#;
    let error = Project::from_toml(text).unwrap_err().to_string();
    assert!(error.contains("depth"), "{error}");
}
