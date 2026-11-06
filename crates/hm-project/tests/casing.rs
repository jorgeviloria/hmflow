//! Sección `[casing]`: lo que una carcasa existente le impone al rodete.

use hm_formula::Registry;
use hm_project::{Casing, Project, PumpType, SCHEMA_VERSION};

const FIXTURE: &str = include_str!("fixtures/casing_v3.hmf");

fn sample() -> Casing {
    Project::from_toml(FIXTURE).unwrap().casing.unwrap()
}

fn codes(casing: Casing) -> Vec<String> {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.casing = Some(casing);
    project
        .validate(&Registry::all())
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect()
}

#[test]
fn fixture_loads_every_field() {
    let project = Project::from_toml(FIXTURE).unwrap();
    assert_eq!(project.schema, SCHEMA_VERSION);
    let casing = project.casing.unwrap();
    assert_eq!(casing.d2, "210 mm");
    assert_eq!(casing.neck_diameter, "90 mm");
    assert_eq!(casing.eye_diameter_max, "75 mm");
    assert_eq!(casing.bore_diameter, "20 mm");
    assert_eq!(casing.hub_diameter, "30 mm");
    assert_eq!(casing.b3, "30 mm");
    assert_eq!(casing.cutwater_radius, "111 mm");
    assert_eq!(casing.channels, 2);
    assert_eq!(casing.wrap, "180 deg");
    assert_eq!(casing.throat_area, "");
}

#[test]
fn a_valid_casing_has_no_diagnostics() {
    assert!(codes(sample()).is_empty());
}

#[test]
fn casing_round_trips_and_stays_out_when_absent() {
    let project = Project::from_toml(FIXTURE).unwrap();
    let text = project.to_toml().unwrap();
    assert!(text.contains("[casing]"));
    assert!(!text.contains("throat_area"), "vacía no se escribe");
    assert_eq!(Project::from_toml(&text).unwrap().casing, Some(sample()));

    let plain = Project::new("demo", PumpType::EndSuction);
    assert!(!plain.to_toml().unwrap().contains("[casing]"));
}

#[test]
fn schema_2_migrates_without_touching_anything() {
    let original = include_str!("fixtures/end_suction_v0.9.hmf");
    assert!(original.starts_with("schema = 2"));
    let project = Project::from_toml(original).unwrap();
    assert_eq!(project.schema, SCHEMA_VERSION);
    assert!(project.casing.is_none());
    assert_eq!(project.params.len(), {
        let same: toml::Value = toml::from_str(original).unwrap();
        same["params"].as_table().unwrap().len()
    });
}

#[test]
fn unknown_casing_field_is_rejected() {
    let text = FIXTURE.replace("[casing]\n", "[casing]\nserial = \"000\"\n");
    assert!(Project::from_toml(&text).is_err());
}

#[test]
fn each_bad_field_is_reported_by_name() {
    type Edit = fn(&mut Casing);
    let cases: [(Edit, &str); 11] = [
        (|c| c.d2 = "300".to_string(), "d2"),
        (|c| c.neck_diameter = "12 kg".to_string(), "neck_diameter"),
        (
            |c| c.eye_diameter_max = "0 mm".to_string(),
            "eye_diameter_max",
        ),
        (|c| c.bore_diameter = "mucho".to_string(), "bore_diameter"),
        (|c| c.hub_diameter = "-30 mm".to_string(), "hub_diameter"),
        (|c| c.b3 = "inf mm".to_string(), "b3"),
        (|c| c.cutwater_radius = "".to_string(), "cutwater_radius"),
        (|c| c.wrap = "180 mm".to_string(), "wrap"),
        (|c| c.wrap = "200 deg".to_string(), "wrap"),
        (|c| c.channels = 3, "channels"),
        (|c| c.throat_area = "20 mm".to_string(), "throat_area"),
    ];
    for (edit, field) in cases {
        let mut casing = sample();
        edit(&mut casing);
        let mut project = Project::new("demo", PumpType::EndSuction);
        project.casing = Some(casing);
        let report = project.validate(&Registry::all());
        let found: Vec<_> = report.iter().collect();
        assert_eq!(found.len(), 1, "{field}: {found:?}");
        assert_eq!(found[0].code, "HM-PROJ-030");
        assert!(
            found[0].message.contains(&format!("`{field}`")),
            "{}",
            found[0].message
        );
    }
}

#[test]
fn the_sizes_must_nest() {
    type Edit = fn(&mut Casing);
    let cases: [Edit; 6] = [
        |c| c.cutwater_radius = "105 mm".to_string(),
        |c| c.neck_diameter = "210 mm".to_string(),
        |c| c.eye_diameter_max = "90 mm".to_string(),
        |c| c.hub_diameter = "75 mm".to_string(),
        |c| c.bore_diameter = "30 mm".to_string(),
        |c| c.cutwater_radius = "104 mm".to_string(),
    ];
    for edit in cases {
        let mut casing = sample();
        edit(&mut casing);
        assert_eq!(codes(casing), ["HM-PROJ-030"]);
    }
}

#[test]
fn the_wrap_goes_with_two_channels_only() {
    let mut missing = sample();
    missing.wrap = String::new();
    assert_eq!(codes(missing), ["HM-PROJ-030"]);

    let mut spare = sample();
    spare.channels = 1;
    assert_eq!(codes(spare), ["HM-PROJ-030"]);

    let mut single = sample();
    single.channels = 1;
    single.wrap = String::new();
    assert!(codes(single.clone()).is_empty());
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.casing = Some(single);
    assert!(!project.to_toml().unwrap().contains("wrap"));
}
