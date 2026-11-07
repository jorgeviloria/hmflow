//! Sección `[meridian]` (esquema 4): los brazos editados del corte meridional.

use hm_formula::Registry;
use hm_project::{Meridian, Project, PumpType, SCHEMA_VERSION};

const CASING_V3: &str = include_str!("fixtures/casing_v3.hmf");

fn with(meridian: Meridian) -> Project {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.meridian = Some(meridian);
    project
}

fn diagnostics(meridian: Meridian) -> Vec<(String, String)> {
    with(meridian)
        .validate(&Registry::all())
        .iter()
        .map(|diagnostic| (diagnostic.code.clone(), diagnostic.message.clone()))
        .collect()
}

#[test]
fn the_schema_is_4() {
    assert_eq!(SCHEMA_VERSION, 4);
    assert_eq!(Project::new("demo", PumpType::EndSuction).schema, 4);
}

#[test]
fn schema_3_migrates_without_touching_anything_else() {
    let project = Project::from_toml(CASING_V3).unwrap();
    assert_eq!(project.schema, SCHEMA_VERSION);
    assert_eq!(project.meridian, None);
    let written = project.to_toml().unwrap();
    assert_eq!(
        written.replace("schema = 4", "schema = 3"),
        Project::from_toml(CASING_V3)
            .unwrap()
            .to_toml()
            .unwrap()
            .replace("schema = 4", "schema = 3")
    );
    assert!(!written.contains("[meridian]"));
}

#[test]
fn edited_arms_round_trip_and_empty_ones_stay_out() {
    let project = with(Meridian {
        front_radial_arm: "20.5 mm".into(),
        rear_axial_arm: "30 mm".into(),
        ..Meridian::default()
    });
    let text = project.to_toml().unwrap();
    assert!(text.contains("[meridian]"), "{text}");
    assert!(text.contains("front_radial_arm = \"20.5 mm\""), "{text}");
    assert!(!text.contains("front_axial_arm"), "{text}");
    let again = Project::from_toml(&text).unwrap();
    assert_eq!(again.meridian, project.meridian);
}

#[test]
fn valid_arms_have_no_diagnostics() {
    let found = diagnostics(Meridian {
        front_axial_arm: "12 mm".into(),
        front_radial_arm: "2 cm".into(),
        rear_axial_arm: "0.03 m".into(),
        rear_radial_arm: "15 mm".into(),
    });
    assert!(
        found.iter().all(|(code, _)| code != "HM-PROJ-031"),
        "{found:?}"
    );
}

#[test]
fn each_bad_arm_is_reported_by_name() {
    for (meridian, name) in [
        (
            Meridian {
                front_axial_arm: "mucho".into(),
                ..Meridian::default()
            },
            "front_axial_arm",
        ),
        (
            Meridian {
                front_radial_arm: "-3 mm".into(),
                ..Meridian::default()
            },
            "front_radial_arm",
        ),
        (
            Meridian {
                rear_axial_arm: "3 kg".into(),
                ..Meridian::default()
            },
            "rear_axial_arm",
        ),
        (
            Meridian {
                rear_radial_arm: "0 mm".into(),
                ..Meridian::default()
            },
            "rear_radial_arm",
        ),
    ] {
        let found = diagnostics(meridian);
        assert!(
            found
                .iter()
                .any(|(code, message)| code == "HM-PROJ-031" && message.contains(name)),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn an_unknown_meridian_field_is_rejected() {
    let text = Project::new("demo", PumpType::EndSuction)
        .to_toml()
        .unwrap()
        + "\n[meridian]\nfront_arm = \"3 mm\"\n";
    assert!(Project::from_toml(&text).is_err());
}
