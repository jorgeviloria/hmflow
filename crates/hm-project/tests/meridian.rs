//! Sección `[meridian]` (esquema 4): lo editado de cada tapa del corte, una
//! Bézier de 5 puntos: P2 y el ángulo y la longitud de los dos brazos.

use hm_formula::Registry;
use hm_project::{
    Constraint, ConstraintKind, Meridian, MeridianEdge, MeridianHubCut, MeridianSegments,
    MeridianWall, MeridianWallSegments, NodeRef, Project, PumpType, SCHEMA_VERSION, WallId,
};

const CASING_V3: &str = include_str!("fixtures/casing_v3.hmf");

fn with(meridian: Meridian) -> Project {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.meridian = Some(meridian);
    project
}

fn problems(meridian: Meridian) -> Vec<String> {
    problems_with("HM-PROJ-031", meridian)
}

fn problems_with(code: &str, meridian: Meridian) -> Vec<String> {
    with(meridian)
        .validate(&Registry::all())
        .iter()
        .filter(|diagnostic| diagnostic.code == code)
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
fn a_new_project_carries_the_current_schema() {
    assert_eq!(SCHEMA_VERSION, 6);
    assert_eq!(
        Project::new("demo", PumpType::EndSuction).schema,
        SCHEMA_VERSION
    );
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

fn texts(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_string()).collect()
}

fn shroud_segments(cuts: &[&str], pieces: &[&str]) -> Meridian {
    Meridian {
        segments: MeridianSegments {
            shroud: MeridianWallSegments {
                cuts: texts(cuts),
                pieces: texts(pieces),
                ..MeridianWallSegments::default()
            },
            ..MeridianSegments::default()
        },
        ..Meridian::default()
    }
}

#[test]
fn segments_round_trip_through_the_hmf() {
    let project = with(shroud_segments(
        &["0.3", "0.65"],
        &["curve", "line", "curve"],
    ));
    let text = project.to_toml().unwrap();
    assert!(text.contains("[meridian.segments.shroud]"), "{text}");
    assert!(!text.contains("segments.hub"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    assert_eq!(back.meridian, project.meridian);
    assert_eq!(problems(project.meridian.unwrap()), Vec::<String>::new());

    let plain = with(Meridian {
        hub_diameter: "20 mm".into(),
        ..Meridian::default()
    });
    assert!(!plain.to_toml().unwrap().contains("segments"));
}

#[test]
fn segments_bad_texts_are_hm_proj_031() {
    let cases = [
        ("segments.shroud.cuts", shroud_segments(&["abc"], &[])),
        ("segments.shroud.cuts", shroud_segments(&["0"], &[])),
        (
            "segments.shroud.cuts",
            shroud_segments(&["0.6", "0.3"], &[]),
        ),
        ("segments.shroud.cuts", shroud_segments(&["1"], &[])),
        (
            "segments.shroud.pieces",
            shroud_segments(&["0.5"], &["line"]),
        ),
        (
            "segments.shroud.pieces",
            shroud_segments(&["0.5"], &["curve", "arco"]),
        ),
    ];
    for (name, meridian) in cases {
        let found = problems(meridian);
        assert!(
            found.iter().any(|message| message.contains(name)),
            "{name}: {found:?}"
        );
    }
    assert_eq!(
        problems(shroud_segments(&["0.5"], &[])),
        Vec::<String>::new()
    );
}

fn hub_nodes(nodes: &[&str]) -> Meridian {
    Meridian {
        segments: MeridianSegments {
            hub: MeridianWallSegments {
                cuts: texts(&["0.5"]),
                pieces: texts(&["curve", "line"]),
                nodes: texts(nodes),
                ..MeridianWallSegments::default()
            },
            ..MeridianSegments::default()
        },
        ..Meridian::default()
    }
}

#[test]
fn free_nodes_round_trip_and_validate() {
    let project = with(hub_nodes(&["", "", "60 mm, 0 mm"]));
    let text = project.to_toml().unwrap();
    assert!(text.contains("60 mm, 0 mm"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    assert_eq!(back.meridian, project.meridian);
    assert_eq!(problems(project.meridian.unwrap()), Vec::<String>::new());
    for nodes in [
        &["", "60 mm, 0 mm"][..],
        &["", "", "60 mm"][..],
        &["", "", "60 mm, -1 mm"][..],
        &["", "", "abc, 0 mm"][..],
    ] {
        let found = problems(hub_nodes(nodes));
        assert!(
            found
                .iter()
                .any(|message| message.contains("segments.hub.nodes")),
            "{nodes:?}: {found:?}"
        );
    }
}

fn hub_radii(radii: &[&str]) -> Meridian {
    Meridian {
        segments: MeridianSegments {
            hub: MeridianWallSegments {
                cuts: texts(&["0.5"]),
                pieces: texts(&["curve", "line"]),
                radii: texts(radii),
                ..MeridianWallSegments::default()
            },
            ..MeridianSegments::default()
        },
        ..Meridian::default()
    }
}

#[test]
fn corner_radii_round_trip_and_validate() {
    let project = with(hub_radii(&["", "5 mm", ""]));
    let text = project.to_toml().unwrap();
    assert!(text.contains("radii"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    assert_eq!(back.meridian, project.meridian);
    assert_eq!(problems(project.meridian.unwrap()), Vec::<String>::new());
    for radii in [
        &["", "5 mm"][..],
        &["", "0 mm", ""][..],
        &["", "-2 mm", ""][..],
        &["", "5", ""][..],
        &["", "abc", ""][..],
    ] {
        let found = problems(hub_radii(radii));
        assert!(
            found
                .iter()
                .any(|message| message.contains("segments.hub.radii")),
            "{radii:?}: {found:?}"
        );
    }
}

fn hub_arcs(pieces: &[&str], arcs: &[&str]) -> Meridian {
    Meridian {
        segments: MeridianSegments {
            hub: MeridianWallSegments {
                cuts: texts(&["0.5"]),
                pieces: texts(pieces),
                arcs: texts(arcs),
                ..MeridianWallSegments::default()
            },
            ..MeridianSegments::default()
        },
        ..Meridian::default()
    }
}

#[test]
fn arcs_round_trip_and_validate() {
    let project = with(hub_arcs(&["arc", "arc"], &["40 mm", "-614 mm"]));
    let text = project.to_toml().unwrap();
    assert!(text.contains("arcs"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    assert_eq!(back.meridian, project.meridian);
    assert_eq!(problems(project.meridian.unwrap()), Vec::<String>::new());
    assert_eq!(
        problems(hub_arcs(&["curve", "line"], &[])),
        Vec::<String>::new()
    );
    for (pieces, arcs) in [
        (&["arc", "line"][..], &[][..]),
        (&["arc", "line"][..], &["40 mm"][..]),
        (&["arc", "line"][..], &["", ""][..]),
        (&["line", "line"][..], &["40 mm", ""][..]),
        (&["arc", "line"][..], &["0 mm", ""][..]),
        (&["arc", "line"][..], &["40", ""][..]),
        (&["arc", "line"][..], &["abc", ""][..]),
    ] {
        let found = problems(hub_arcs(pieces, arcs));
        assert!(
            found
                .iter()
                .any(|message| message.contains("segments.hub.arcs")),
            "{pieces:?} {arcs:?}: {found:?}"
        );
    }
}

fn hub_tangents(pieces: &[&str], arcs: &[&str], tangents: &[&str]) -> Meridian {
    Meridian {
        segments: MeridianSegments {
            hub: MeridianWallSegments {
                cuts: texts(&["0.5"]),
                pieces: texts(pieces),
                arcs: texts(arcs),
                tangents: texts(tangents),
                ..MeridianWallSegments::default()
            },
            ..MeridianSegments::default()
        },
        ..Meridian::default()
    }
}

#[test]
fn tangents_round_trip_and_validate() {
    let project = with(hub_tangents(
        &["line", "arc"],
        &["", "614 mm"],
        &["", "start"],
    ));
    let text = project.to_toml().unwrap();
    assert!(text.contains("tangents"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    assert_eq!(back.meridian, project.meridian);
    assert_eq!(problems(project.meridian.unwrap()), Vec::<String>::new());
    assert_eq!(
        problems(hub_tangents(&["arc", "line"], &["40 mm", ""], &["end", ""])),
        Vec::<String>::new()
    );
    for (pieces, arcs, tangents) in [
        // Uno por tramo.
        (&["line", "arc"][..], &["", "40 mm"][..], &["start"][..]),
        // Solo en arcos.
        (&["line", "arc"][..], &["", "40 mm"][..], &["start", ""][..]),
        // Sin vecino de ese lado.
        (&["arc", "line"][..], &["40 mm", ""][..], &["start", ""][..]),
        (&["line", "arc"][..], &["", "40 mm"][..], &["", "end"][..]),
        // Palabra desconocida.
        (
            &["line", "arc"][..],
            &["", "40 mm"][..],
            &["", "inicio"][..],
        ),
    ] {
        let found = problems(hub_tangents(pieces, arcs, tangents));
        assert!(
            found
                .iter()
                .any(|message| message.contains("segments.hub.tangents")),
            "{pieces:?} {tangents:?}: {found:?}"
        );
    }
}

/// Hub con nodos 0–3 (tramos curve, line, line) y shroud con nodos 0–2
/// (tramos curve, line), más las cotas que se pasan.
fn with_constraints(items: &[&str]) -> Meridian {
    Meridian {
        segments: MeridianSegments {
            hub: MeridianWallSegments {
                cuts: texts(&["0.3", "0.6"]),
                pieces: texts(&["curve", "line", "line"]),
                ..MeridianWallSegments::default()
            },
            shroud: MeridianWallSegments {
                cuts: texts(&["0.5"]),
                pieces: texts(&["curve", "line"]),
                ..MeridianWallSegments::default()
            },
            constraints: texts(items),
        },
        ..Meridian::default()
    }
}

fn problems_032(meridian: Meridian) -> Vec<String> {
    problems_with("HM-PROJ-032", meridian)
}

#[test]
fn constraint_text_parses_and_formats() {
    let c = Constraint::parse("dz shroud:2 hub:3 = 40 mm").unwrap();
    assert_eq!(c.kind, ConstraintKind::Dz);
    assert_eq!(
        c.parent,
        NodeRef {
            wall: WallId::Shroud,
            index: 2
        }
    );
    assert_eq!(
        c.child,
        NodeRef {
            wall: WallId::Hub,
            index: 3
        }
    );
    assert!((c.value - 0.040).abs() < 1e-12);
    assert_eq!(c.to_text(), "dz shroud:2 hub:3 = 40 mm");
    let a = Constraint::parse("ang hub:2 hub:3 = 135 deg").unwrap();
    assert_eq!(a.kind, ConstraintKind::Ang);
    assert!((a.value - 135.0).abs() < 1e-12);
    assert_eq!(a.to_text(), "ang hub:2 hub:3 = 135 deg");
    assert_eq!(
        Constraint::parse("dd hub:1 hub:2 = -3 cm")
            .unwrap()
            .to_text(),
        "dd hub:1 hub:2 = -30 mm"
    );
}

#[test]
fn constraint_text_tolerates_spacing() {
    assert_eq!(
        Constraint::parse("  dz  hub:1   hub:2 =40 mm ").unwrap(),
        Constraint::parse("dz hub:1 hub:2 = 40 mm").unwrap()
    );
}

#[test]
fn constraints_round_trip_through_the_hmf() {
    let project = with(with_constraints(&[
        "dz shroud:2 hub:3 = 40 mm",
        "ang hub:2 hub:3 = 135 deg",
    ]));
    let text = project.to_toml().unwrap();
    assert!(text.contains("constraints"), "{text}");
    assert_eq!(
        Project::from_toml(&text).unwrap().meridian,
        project.meridian
    );
}

#[test]
fn constraints_stay_out_when_empty() {
    let text = with(with_constraints(&[])).to_toml().unwrap();
    assert!(!text.contains("constraints"), "{text}");
}

#[test]
fn constraints_valid_set_has_no_problems() {
    assert_eq!(
        problems_032(with_constraints(&[
            "dz shroud:2 hub:3 = 40 mm",
            // Cadena shroud:2 → hub:3 → hub:2, sin ciclo.
            "dd hub:3 hub:2 = 12 mm",
            // El tramo 1 (nodos 1–2) es line; límite superior del ángulo.
            "ang hub:1 hub:2 = 180 deg",
            "dz hub:0 hub:1 = 5 mm",
        ])),
        Vec::<String>::new()
    );
}

#[test]
fn constraints_bad_texts_are_hm_proj_032() {
    for items in [
        // Falta «=».
        &["dz hub:1 hub:2 40 mm"][..],
        // Tipo desconocido.
        &["dx hub:1 hub:2 = 40 mm"][..],
        // Tapa desconocida.
        &["dz tapa:1 hub:2 = 40 mm"][..],
        // Nodo inexistente.
        &["dz hub:1 hub:9 = 40 mm"][..],
        // dz sin longitud.
        &["dz hub:1 hub:2 = 40 deg"][..],
        // ang sin deg.
        &["ang hub:1 hub:2 = 40 mm"][..],
        // Fuera de (−180, 180].
        &["ang hub:1 hub:2 = -180 deg"][..],
        // Padre = hijo.
        &["dz hub:2 hub:2 = 1 mm"][..],
        // Ciclo entre tapas.
        &["dz hub:1 shroud:1 = 1 mm", "dd shroud:1 hub:1 = 1 mm"][..],
        // La z con dos dueños.
        &["dz hub:1 hub:2 = 1 mm", "dz hub:3 hub:2 = 2 mm"][..],
        // Tres dueños.
        &[
            "dz hub:1 hub:2 = 1 mm",
            "dd hub:1 hub:2 = 1 mm",
            "ang hub:1 hub:2 = 45 deg",
        ][..],
        // El tramo 0 es curve.
        &["ang hub:0 hub:1 = 45 deg"][..],
        // No son vecinos.
        &["ang hub:1 hub:3 = 45 deg"][..],
    ] {
        let found = problems_032(with_constraints(items));
        assert!(
            found
                .iter()
                .any(|message| message.contains("segments.constraints")),
            "{items:?}: {found:?}"
        );
    }
    // Tapa sin cortes: solo existen los nodos 0 y 1.
    let mut meridian = with_constraints(&["dz hub:0 hub:2 = 1 mm"]);
    meridian.segments.hub = MeridianWallSegments::default();
    assert!(!problems_032(meridian).is_empty());
}

#[test]
fn over_determined_child_is_named_neutrally() {
    let found = problems_032(with_constraints(&[
        "ang hub:1 hub:2 = 45 deg",
        "dz hub:1 hub:2 = 1 mm",
        "dd hub:1 hub:2 = 1 mm",
    ]));
    assert!(
        found
            .iter()
            .any(|message| message.contains("dz, dd y ang a la vez")),
        "{found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|message| message.contains("el ángulo sobra")),
        "{found:?}"
    );
}

#[test]
fn constraint_text_tolerates_spaces_around_colons_and_case() {
    let plain = Constraint::parse("dz hub:1 hub:2 = 40 mm").unwrap();
    assert_eq!(
        Constraint::parse("dz hub: 1 hub :2 = 40 mm").unwrap(),
        plain
    );
    assert_eq!(Constraint::parse("DZ Hub:1 HUB:2 = 40 mm").unwrap(), plain);
    assert_eq!(
        Constraint::parse("Ang hub:1 hub:2 = 135 DEG").unwrap(),
        Constraint::parse("ang hub:1 hub:2 = 135 deg").unwrap()
    );
}

#[test]
fn angles_out_of_range_or_not_numbers_are_rejected() {
    for value in ["180.0000001 deg", "NaN deg", "inf deg", "-inf deg", "deg"] {
        let text = format!("ang hub:1 hub:2 = {value}");
        assert!(Constraint::parse(&text).is_err(), "{text}");
    }
}

fn extended_segments(
    head: &str,
    cuts: &[&str],
    pieces: &[&str],
    nodes: &[&str],
    tail: &str,
) -> Meridian {
    Meridian {
        segments: MeridianSegments {
            shroud: MeridianWallSegments {
                head: head.to_string(),
                tail: tail.to_string(),
                cuts: texts(cuts),
                pieces: texts(pieces),
                nodes: texts(nodes),
                ..MeridianWallSegments::default()
            },
            ..MeridianSegments::default()
        },
        ..Meridian::default()
    }
}

#[test]
fn extension_round_trips_through_the_hmf() {
    // head = 2, sin cortes: 4 nodos y 3 tramos.
    let meridian = extended_segments(
        "2",
        &[],
        &["line", "line", "curve"],
        &["10 mm, 0 mm", "10 mm, 20 mm", "", ""],
        "",
    );
    let project = with(meridian);
    let text = project.to_toml().unwrap();
    assert!(text.contains("head = \"2\""), "{text}");
    assert!(!text.contains("tail"), "{text}");
    assert_eq!(
        Project::from_toml(&text).unwrap().meridian,
        project.meridian
    );
    assert_eq!(problems(project.meridian.unwrap()), Vec::<String>::new());
}

#[test]
fn old_segments_without_extension_still_load() {
    let project = with(shroud_segments(&["0.3"], &["curve", "line"]));
    let text = project.to_toml().unwrap();
    assert!(!text.contains("head") && !text.contains("tail"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    let wall = &back.meridian.as_ref().unwrap().segments.shroud;
    assert_eq!((wall.head.as_str(), wall.tail.as_str()), ("", ""));
    assert_eq!(wall.node_count(), 3);
    assert_eq!(problems(back.meridian.unwrap()), Vec::<String>::new());
}

#[test]
fn bad_extensions_are_hm_proj_031() {
    let cases = [
        (
            "segments.shroud.head",
            extended_segments("-1", &[], &[], &[], ""),
        ),
        (
            "segments.shroud.head",
            extended_segments("dos", &[], &[], &[], ""),
        ),
        (
            "segments.shroud.pieces",
            extended_segments("1", &[], &["curve", "curve"], &["1 mm, 1 mm", "", ""], ""),
        ),
        (
            "segments.shroud.pieces",
            extended_segments("1", &[], &[], &[], ""),
        ),
        (
            "segments.shroud.nodes",
            extended_segments("1", &[], &["line", "curve"], &["", "", ""], ""),
        ),
        (
            "segments.shroud.nodes",
            extended_segments("1", &[], &["line", "curve"], &["1 mm, 1 mm", ""], ""),
        ),
        (
            "segments.shroud.nodes",
            extended_segments("1", &[], &["line", "curve"], &[], ""),
        ),
        (
            "segments.shroud.tail",
            extended_segments("", &[], &["curve", "curve"], &["", "", "1 mm, 1 mm"], "x"),
        ),
    ];
    for (name, meridian) in cases {
        let found = problems(meridian);
        assert!(
            found.iter().any(|message| message.contains(name)),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn constraint_on_an_extension_node_exists() {
    let mut meridian = extended_segments("1", &[], &["line", "curve"], &["1 mm, 1 mm", "", ""], "");
    // Con head = 1 hay 3 nodos (0..=2): el 2 existe, el 3 no.
    meridian.segments.constraints = texts(&["dz shroud:0 shroud:1 = 5 mm"]);
    assert_eq!(
        problems_with("HM-PROJ-032", meridian.clone()),
        Vec::<String>::new()
    );
    meridian.segments.constraints = texts(&["dz shroud:0 shroud:3 = 5 mm"]);
    assert!(!problems_with("HM-PROJ-032", meridian).is_empty());
}

fn closed_segments(head: &str, tail: &str, closed: &str) -> Meridian {
    // head/tail de un nodo cada uno: nodos 0 y 3 libres, 3 tramos (rectas y curva).
    let (pieces, nodes): (&[&str], &[&str]) = match (head, tail) {
        ("1", "1") => (
            &["line", "curve", "line"],
            &["1 mm, 0 mm", "", "", "9 mm, 0 mm"],
        ),
        _ => (&[], &[]),
    };
    let mut meridian = extended_segments(head, &[], pieces, nodes, tail);
    meridian.segments.shroud.closed = closed.to_string();
    meridian
}

#[test]
fn closed_round_trips_through_the_hmf() {
    let project = with(closed_segments("1", "1", "true"));
    let text = project.to_toml().unwrap();
    assert!(text.contains("closed = \"true\""), "{text}");
    let back = Project::from_toml(&text).unwrap();
    assert_eq!(back.meridian, project.meridian);
    assert!(back.meridian.as_ref().unwrap().segments.shroud.is_closed());
    assert_eq!(problems(project.meridian.unwrap()), Vec::<String>::new());
}

#[test]
fn segments_without_closed_still_load() {
    let project = with(shroud_segments(&["0.3"], &["curve", "line"]));
    let text = project.to_toml().unwrap();
    assert!(!text.contains("closed"), "{text}");
    let back = Project::from_toml(&text).unwrap();
    assert!(!back.meridian.as_ref().unwrap().segments.shroud.is_closed());
}

#[test]
fn bad_closed_is_hm_proj_031() {
    let cases = [
        ("segments.shroud.closed", closed_segments("1", "1", "si")),
        ("segments.shroud.closed", closed_segments("", "", "true")),
    ];
    for (name, meridian) in cases {
        let found = problems(meridian);
        assert!(
            found.iter().any(|message| message.contains(name)),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn the_edge_shape_round_trips_and_is_checked() {
    let line = with(Meridian {
        edge_shape: "line".into(),
        ..Meridian::default()
    });
    let written = line.to_toml().unwrap();
    assert!(written.contains("edge_shape = \"line\""), "{written}");
    let back = Project::from_toml(&written).unwrap();
    assert_eq!(back.meridian.as_ref().unwrap().edge_shape, "line");
    // Vacío no se escribe, y un meridiano con solo la forma no está vacío.
    assert!(Meridian::default().is_empty());
    assert!(
        !Meridian {
            edge_shape: "line".into(),
            ..Meridian::default()
        }
        .is_empty()
    );
    for good in ["", "channel", "line", " line "] {
        let ok = problems(Meridian {
            edge_shape: good.into(),
            ..Meridian::default()
        });
        assert!(ok.is_empty(), "{good:?}: {ok:?}");
    }
    let bad = problems(Meridian {
        edge_shape: "spline".into(),
        ..Meridian::default()
    });
    assert!(bad.iter().any(|m| m.contains("edge_shape")), "{bad:?}");
}
