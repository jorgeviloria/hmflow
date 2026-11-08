//! Sección `[meridian]` (esquema 4): lo editado de cada tapa del corte, una
//! Bézier de 5 puntos: P2 y el ángulo y la longitud de los dos brazos.

use hm_formula::Registry;
use hm_project::{
    Meridian, MeridianEdge, MeridianHubCut, MeridianWall, Project, PumpType, SCHEMA_VERSION,
};

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
        hub_diameter: "0 mm".into(),
        edge_shroud: "0.2".into(),
        edge_hub: "0.35".into(),
        edge: MeridianEdge {
            p2_along: "0.4".into(),
            p2_offset: "-2.5 mm".into(),
            start_angle_deg: "20".into(),
            ..MeridianEdge::default()
        },
        shroud: MeridianWall {
            p2_z: "24 mm".into(),
            p2_r: "60 mm".into(),
            start_angle_deg: "12.5".into(),
            ..MeridianWall::default()
        },
        hub: MeridianWall {
            end_length: "4 mm".into(),
            ..MeridianWall::default()
        },
        ..Meridian::default()
    });
    let text = project.to_toml().unwrap();
    assert!(text.contains("hub_diameter = \"0 mm\""), "{text}");
    assert!(text.contains("edge_shroud = \"0.2\""), "{text}");
    assert!(text.contains("edge_hub = \"0.35\""), "{text}");
    assert!(text.contains("[meridian.edge]"), "{text}");
    assert!(text.contains("p2_along = \"0.4\""), "{text}");
    assert!(text.contains("p2_offset = \"-2.5 mm\""), "{text}");
    assert!(text.contains("start_angle_deg = \"20\""), "{text}");
    assert!(text.contains("[meridian.shroud]"), "{text}");
    assert!(text.contains("p2_z = \"24 mm\""), "{text}");
    assert!(text.contains("[meridian.hub]"), "{text}");
    assert!(text.contains("start_angle_deg = \"12.5\""), "{text}");
    assert!(!text.contains("end_length = \"\""), "{text}");
    assert_eq!(
        Project::from_toml(&text).unwrap().meridian,
        project.meridian
    );

    let only_hub = with(Meridian {
        hub: MeridianWall {
            start_length: "5 mm".into(),
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
            start_angle_deg: "-90".into(),
            start_length: "0.5 mm".into(),
            end_angle_deg: "90".into(),
            end_length: "3 cm".into(),
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
                start_angle_deg: "91".into(),
                ..MeridianWall::default()
            },
            "shroud.start_angle_deg",
        ),
        (
            MeridianWall {
                end_angle_deg: "recto".into(),
                ..MeridianWall::default()
            },
            "shroud.end_angle_deg",
        ),
        (
            MeridianWall {
                start_length: "0 mm".into(),
                ..MeridianWall::default()
            },
            "shroud.start_length",
        ),
        (
            MeridianWall {
                end_length: "5".into(),
                ..MeridianWall::default()
            },
            "shroud.end_length",
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

#[test]
fn the_hub_diameter_and_the_edge_positions_are_checked() {
    let bad = |meridian: Meridian, name: &str| {
        problems(meridian)
            .iter()
            .any(|message| message.contains(name))
    };
    for (text, ok) in [
        ("0 mm", true),
        ("30 mm", true),
        ("-1 mm", false),
        ("ancho", false),
    ] {
        let meridian = Meridian {
            hub_diameter: text.into(),
            ..Meridian::default()
        };
        assert_eq!(bad(meridian, "hub_diameter"), !ok, "{text}");
    }
    for (text, ok) in [
        ("0", true),
        ("0.9", true),
        ("-0.1", false),
        ("0.95", false),
        ("x", false),
    ] {
        let shroud = Meridian {
            edge_shroud: text.into(),
            ..Meridian::default()
        };
        let hub = Meridian {
            edge_hub: text.into(),
            ..Meridian::default()
        };
        assert_eq!(bad(shroud, "edge_shroud"), !ok, "{text}");
        assert_eq!(bad(hub, "edge_hub"), !ok, "{text}");
    }
}

#[test]
fn the_edge_fields_are_checked_like_the_walls() {
    let bad = |edge: MeridianEdge, name: &str| {
        problems(Meridian {
            edge,
            ..Meridian::default()
        })
        .iter()
        .any(|message| message.contains(name))
    };
    let along = |text: &str| MeridianEdge {
        p2_along: text.into(),
        ..MeridianEdge::default()
    };
    let offset = |text: &str| MeridianEdge {
        p2_offset: text.into(),
        ..MeridianEdge::default()
    };
    let angle = |text: &str| MeridianEdge {
        start_angle_deg: text.into(),
        end_angle_deg: text.into(),
        ..MeridianEdge::default()
    };
    let length = |text: &str| MeridianEdge {
        start_length: text.into(),
        end_length: text.into(),
        ..MeridianEdge::default()
    };
    for (text, ok) in [
        ("0", true),
        ("1", true),
        ("0.4", true),
        ("1.1", false),
        ("-0.1", false),
        ("x", false),
    ] {
        assert_eq!(bad(along(text), "edge.p2_along"), !ok, "{text}");
    }
    // El desvío lleva unidad y signo.
    for (text, ok) in [
        ("0 mm", true),
        ("5 mm", true),
        ("-3 mm", true),
        ("5", false),
        ("x", false),
    ] {
        assert_eq!(bad(offset(text), "edge.p2_offset"), !ok, "{text}");
    }
    for (text, ok) in [("-90", true), ("45.5", true), ("91", false), ("x", false)] {
        assert_eq!(bad(angle(text), "edge.start_angle_deg"), !ok, "{text}");
        assert_eq!(bad(angle(text), "edge.end_angle_deg"), !ok, "{text}");
    }
    for (text, ok) in [
        ("4 mm", true),
        ("0 mm", false),
        ("-1 mm", false),
        ("4", false),
    ] {
        assert_eq!(bad(length(text), "edge.start_length"), !ok, "{text}");
        assert_eq!(bad(length(text), "edge.end_length"), !ok, "{text}");
    }
}

fn full_cut() -> MeridianHubCut {
    MeridianHubCut {
        z: "12 mm".into(),
        mode: "through".into(),
        shaft_diameter: "25 mm".into(),
        counterbore_diameter: "40 mm".into(),
        counterbore_depth: "6 mm".into(),
        chamfer: "1 mm".into(),
    }
}

#[test]
fn hub_cut_round_trips_through_the_hmf() {
    let project = with(Meridian {
        hub_cut: full_cut(),
        ..Meridian::default()
    });
    let text = project.to_toml().unwrap();
    assert!(text.contains("[meridian.hub_cut]"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    assert_eq!(back.meridian, project.meridian);

    let empty = with(Meridian {
        hub_diameter: "20 mm".into(),
        ..Meridian::default()
    });
    assert!(!empty.to_toml().unwrap().contains("hub_cut"));
}

#[test]
fn hub_cut_bad_texts_are_hm_proj_031() {
    let cases: [(&str, MeridianHubCut); 4] = [
        (
            "hub_cut.z",
            MeridianHubCut {
                z: "abc".into(),
                ..full_cut()
            },
        ),
        (
            "hub_cut.shaft_diameter",
            MeridianHubCut {
                shaft_diameter: "-3 mm".into(),
                ..full_cut()
            },
        ),
        (
            "hub_cut.mode",
            MeridianHubCut {
                mode: "otro".into(),
                ..full_cut()
            },
        ),
        (
            "hub_cut.counterbore_depth",
            MeridianHubCut {
                counterbore_depth: "0 mm".into(),
                ..full_cut()
            },
        ),
    ];
    for (name, cut) in cases {
        let found = problems(Meridian {
            hub_cut: cut,
            ..Meridian::default()
        });
        assert!(
            found.iter().any(|message| message.contains(name)),
            "{name}: {found:?}"
        );
    }
    assert_eq!(
        problems(Meridian {
            hub_cut: full_cut(),
            ..Meridian::default()
        }),
        Vec::<String>::new()
    );
}
