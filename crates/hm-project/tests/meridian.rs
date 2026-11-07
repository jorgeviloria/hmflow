//! Sección `[meridian]` (esquema 4): lo editado de cada tapa del corte, una
//! Bézier de 5 puntos: P2 y las fracciones de P1 y P3.

use hm_formula::Registry;
use hm_project::{Meridian, MeridianWall, Project, PumpType, SCHEMA_VERSION};

const CASING_V3: &str = include_str!("fixtures/casing_v3.hmf");

fn with(meridian: Meridian) -> Project {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.meridian = Some(meridian);
    project
}

fn problems(meridian: Meridian) -> Vec<String> {
    with(meridian)
        .validate(&Registry::all())
        .iter()
        .filter(|diagnostic| diagnostic.code == "HM-PROJ-031")
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

fn shroud(wall: MeridianWall) -> Meridian {
    Meridian {
        shroud: wall,
        ..Meridian::default()
    }
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
    assert!(!written.contains("[meridian"));
    assert_eq!(
        Project::from_toml(&written).unwrap().to_toml().unwrap(),
        written
    );
}

#[test]
fn edits_round_trip_and_empty_fields_stay_out() {
    let project = with(Meridian {
        shroud: MeridianWall {
            p2_z: "24 mm".into(),
            p2_r: "60 mm".into(),
            p1: "0.3".into(),
            ..MeridianWall::default()
        },
        hub: MeridianWall {
            p3: "0.4".into(),
            ..MeridianWall::default()
        },
    });
    let text = project.to_toml().unwrap();
    assert!(text.contains("[meridian.shroud]"), "{text}");
    assert!(text.contains("p2_z = \"24 mm\""), "{text}");
    assert!(text.contains("[meridian.hub]"), "{text}");
    assert!(!text.contains("p3 = \"\""), "{text}");
    assert_eq!(
        Project::from_toml(&text).unwrap().meridian,
        project.meridian
    );

    let only_hub = with(Meridian {
        hub: MeridianWall {
            p1: "0.5".into(),
            ..MeridianWall::default()
        },
        ..Meridian::default()
    });
    assert!(!only_hub.to_toml().unwrap().contains("[meridian.shroud]"));
}

#[test]
fn valid_edits_have_no_problems() {
    assert_eq!(
        problems(shroud(MeridianWall {
            p2_z: "-2 mm".into(),
            p2_r: "6 cm".into(),
            p1: "0.02".into(),
            p3: "0.98".into(),
        })),
        Vec::<String>::new()
    );
}

#[test]
fn each_bad_field_is_reported_by_name() {
    for (wall, name) in [
        (
            MeridianWall {
                p2_z: "24 mm".into(),
                ..MeridianWall::default()
            },
            "shroud.p2",
        ),
        (
            MeridianWall {
                p2_z: "mucho".into(),
                p2_r: "60 mm".into(),
                ..MeridianWall::default()
            },
            "shroud.p2_z",
        ),
        (
            MeridianWall {
                p2_z: "20 mm".into(),
                p2_r: "-3 mm".into(),
                ..MeridianWall::default()
            },
            "shroud.p2_r",
        ),
        (
            MeridianWall {
                p1: "1".into(),
                ..MeridianWall::default()
            },
            "shroud.p1",
        ),
        (
            MeridianWall {
                p3: "medio".into(),
                ..MeridianWall::default()
            },
            "shroud.p3",
        ),
    ] {
        let found = problems(shroud(wall));
        assert!(
            found.iter().any(|message| message.contains(name)),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn an_unknown_meridian_field_is_rejected() {
    let text = Project::new("demo", PumpType::EndSuction)
        .to_toml()
        .unwrap()
        + "\n[meridian.shroud]\nfront_arm = \"3 mm\"\n";
    assert!(Project::from_toml(&text).is_err());
}
