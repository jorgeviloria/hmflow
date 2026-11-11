//! Sección `[blade]`: la forma del álabe y el número de líneas de corriente
//! (esquema 5; las manijas, esquema 6; el espesor, esquema 7).

use hm_project::{
    BLADE_BOW_RANGE_DEG, BLADE_SHAPES, Blade, DEFAULT_BLADE_SHAPE, DEFAULT_STREAMLINES,
    MIGRATABLE_SCHEMAS, Project, PumpType, SCHEMA_VERSION, STREAMLINE_COUNTS,
};

const V4: &str = include_str!("fixtures/end_suction_v4.hmf");
const V5: &str = include_str!("fixtures/end_suction_v5.hmf");
const V6: &str = include_str!("fixtures/end_suction_v6.hmf");

/// Un proyecto nuevo con esa sección `[blade]` añadida al final.
fn with_blade(section: &str) -> String {
    let mut text = Project::new("demo", PumpType::EndSuction)
        .to_toml()
        .unwrap();
    text.push_str("\n[blade]\n");
    text.push_str(section);
    text.push('\n');
    text
}

#[test]
fn a_v4_file_without_blade_loads_as_default_and_saves_as_current_without_the_section() {
    assert!(V4.starts_with("schema = 4\n"), "la fixture debe ser v4");
    let project = Project::from_toml(V4).unwrap();
    assert_eq!(project.schema, SCHEMA_VERSION);
    assert_eq!(project.blade, None);
    let saved = project.to_toml().unwrap();
    assert!(saved.starts_with("schema = 7\n"), "{saved}");
    assert!(!saved.contains("[blade]"), "{saved}");
}

#[test]
fn a_non_default_blade_round_trips() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    let blade = Blade {
        shape: "free_2d".into(),
        streamlines: 9,
        ..Blade::default()
    };
    project.blade = Some(blade.clone());
    let saved = project.to_toml().unwrap();
    assert!(saved.contains("[blade]"), "{saved}");
    assert_eq!(Project::from_toml(&saved).unwrap().blade, Some(blade));
}

#[test]
fn a_default_blade_is_not_written() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    assert!(Blade::default().is_default());
    project.blade = Some(Blade::default());
    let saved = project.to_toml().unwrap();
    assert!(!saved.contains("[blade]"), "{saved}");
}

#[test]
fn a_partial_blade_section_takes_the_other_default() {
    let only_count = Project::from_toml(&with_blade("streamlines = 7")).unwrap();
    assert_eq!(
        only_count.blade,
        Some(Blade {
            shape: DEFAULT_BLADE_SHAPE.into(),
            streamlines: 7,
            ..Blade::default()
        })
    );
    let only_shape = Project::from_toml(&with_blade("shape = \"free_2d\"")).unwrap();
    assert_eq!(only_shape.blade.unwrap().streamlines, DEFAULT_STREAMLINES);
}

#[test]
fn an_invalid_streamline_count_is_a_load_error() {
    for count in [4, 11] {
        let error = Project::from_toml(&with_blade(&format!("streamlines = {count}")))
            .unwrap_err()
            .to_string();
        assert!(error.contains("debe ser 3, 5, 7 o 9"), "{error}");
        assert!(error.contains(&count.to_string()), "{error}");
    }
}

#[test]
fn an_unknown_shape_is_a_load_error() {
    let error = Project::from_toml(&with_blade("shape = \"foo\""))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("forma de álabe desconocida «foo»"),
        "{error}"
    );
}

#[test]
fn every_listed_shape_loads() {
    for shape in BLADE_SHAPES {
        let project = Project::from_toml(&with_blade(&format!("shape = \"{shape}\""))).unwrap();
        assert_eq!(project.blade.unwrap().shape, shape);
    }
}

#[test]
fn an_unknown_field_in_the_section_is_rejected() {
    assert!(Project::from_toml(&with_blade("lines = 5")).is_err());
}

#[test]
fn the_schema_is_seven_and_migrates_one_to_six() {
    assert_eq!(SCHEMA_VERSION, 7);
    assert_eq!(MIGRATABLE_SCHEMAS, [1, 2, 3, 4, 5, 6]);
    assert_eq!(BLADE_BOW_RANGE_DEG, (-30.0, 30.0));
    assert_eq!(STREAMLINE_COUNTS, [3, 5, 7, 9]);
    assert_eq!(DEFAULT_STREAMLINES, 5);
    assert_eq!(DEFAULT_BLADE_SHAPE, "free_2d");
}

#[test]
fn a_v5_file_loads_with_zero_handles_and_saves_as_current() {
    assert!(V5.starts_with("schema = 5\n"), "la fixture debe ser v5");
    let project = Project::from_toml(V5).unwrap();
    assert_eq!(project.schema, SCHEMA_VERSION);
    let blade = project.blade.as_ref().unwrap();
    assert_eq!(blade.bow_hub, [0.0, 0.0]);
    assert_eq!(blade.bow_shroud, [0.0, 0.0]);
    assert_eq!(blade.streamlines, 7);
    let saved = project.to_toml().unwrap();
    assert!(saved.starts_with("schema = 7\n"), "{saved}");
    assert!(!saved.contains("bow_"), "{saved}");
}

#[test]
fn handles_round_trip() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    let blade = Blade {
        shape: "free_3d".into(),
        streamlines: 5,
        bow_hub: [3.5, -2.0],
        bow_shroud: [0.0, 1.25],
        ..Blade::default()
    };
    project.blade = Some(blade.clone());
    let saved = project.to_toml().unwrap();
    assert!(saved.contains("bow_hub"), "{saved}");
    assert!(saved.contains("bow_shroud"), "{saved}");
    assert_eq!(Project::from_toml(&saved).unwrap().blade, Some(blade));
}

#[test]
fn zero_handles_are_not_written_and_a_handle_makes_the_blade_non_default() {
    let mut blade = Blade::default();
    assert!(blade.is_default());
    blade.bow_shroud = [0.0, 1.0];
    assert!(!blade.is_default());
    // Solo la manija se escribe; la que es cero se omite.
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.blade = Some(blade);
    let saved = project.to_toml().unwrap();
    assert!(saved.contains("bow_shroud"), "{saved}");
    assert!(!saved.contains("bow_hub"), "{saved}");
}

#[test]
fn a_half_written_blade_loads_with_defaults() {
    let only_hub = Project::from_toml(&with_blade("bow_hub = [1.0, 2.0]")).unwrap();
    let blade = only_hub.blade.unwrap();
    assert_eq!(blade.shape, DEFAULT_BLADE_SHAPE);
    assert_eq!(blade.streamlines, DEFAULT_STREAMLINES);
    assert_eq!(blade.bow_hub, [1.0, 2.0]);
    assert_eq!(blade.bow_shroud, [0.0, 0.0]);

    // Manijas con la forma 2D: cargan y se conservan para cuando se vuelva a 3D.
    let kept =
        Project::from_toml(&with_blade("shape = \"free_2d\"\nbow_shroud = [4.0, -4.0]")).unwrap();
    let blade = kept.blade.unwrap();
    assert_eq!(blade.shape, "free_2d");
    assert_eq!(blade.bow_shroud, [4.0, -4.0]);
    let saved = Project::from_toml(&with_blade("bow_shroud = [4.0, -4.0]"))
        .unwrap()
        .to_toml()
        .unwrap();
    assert!(saved.contains("bow_shroud"), "{saved}");
}

#[test]
fn a_handle_out_of_range_is_a_load_error() {
    for text in [
        "bow_hub = [31.0, 0.0]",
        "bow_shroud = [0.0, -30.5]",
        "bow_hub = [nan, 0.0]",
        "bow_hub = [inf, 0.0]",
        "bow_shroud = [1.0]",
        "bow_shroud = [1.0, 2.0, 3.0]",
    ] {
        let error = Project::from_toml(&with_blade(text))
            .unwrap_err()
            .to_string();
        let field = if text.starts_with("bow_hub") {
            "bow_hub"
        } else {
            "bow_shroud"
        };
        assert!(error.contains(field), "{text}: {error}");
        assert!(
            error.contains("-30") && error.contains("30"),
            "{text}: {error}"
        );
    }
}

#[test]
fn a_v6_file_loads_without_thickness_and_saves_as_v7() {
    assert!(V6.starts_with("schema = 6\n"), "la fixture debe ser v6");
    let project = Project::from_toml(V6).unwrap();
    assert_eq!(project.schema, SCHEMA_VERSION);
    let blade = project.blade.as_ref().unwrap();
    assert_eq!(blade.bow_hub, [3.5, -2.0]);
    assert!(blade.thickness_same);
    assert!(blade.thickness_hub.is_empty());
    assert!(blade.thickness_shroud.is_empty());
    let saved = project.to_toml().unwrap();
    assert!(saved.starts_with("schema = 7\n"), "{saved}");
    assert!(!saved.contains("thickness"), "{saved}");
    assert!(saved.contains("bow_hub"), "{saved}");
}

#[test]
fn thickness_round_trips() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    let blade = Blade {
        shape: "free_3d".into(),
        thickness_same: false,
        thickness_hub: vec![[0.0, 6.0], [0.8, 6.0], [1.0, 3.0]],
        thickness_shroud: vec![[0.0, 5.0], [0.3, 5.5], [0.6, 4.0], [0.9, 3.5], [1.0, 2.0]],
        ..Blade::default()
    };
    project.blade = Some(blade.clone());
    let saved = project.to_toml().unwrap();
    assert!(saved.contains("thickness_same = false"), "{saved}");
    assert!(saved.contains("thickness_hub"), "{saved}");
    assert!(saved.contains("thickness_shroud"), "{saved}");
    assert_eq!(Project::from_toml(&saved).unwrap().blade, Some(blade));

    // Con «cubo = tapa» solo se escribe el cubo, y el interruptor encendido no se escribe.
    let blade = Blade {
        thickness_hub: vec![[0.0, 6.0], [1.0, 3.0]],
        ..Blade::default()
    };
    project.blade = Some(blade.clone());
    let saved = project.to_toml().unwrap();
    assert!(!saved.contains("thickness_same"), "{saved}");
    assert!(!saved.contains("thickness_shroud"), "{saved}");
    assert_eq!(Project::from_toml(&saved).unwrap().blade, Some(blade));
}

#[test]
fn default_thickness_is_not_written_and_nodes_make_the_blade_non_default() {
    let mut blade = Blade::default();
    assert!(blade.is_default());
    assert!(blade.thickness_same);
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.blade = Some(blade.clone());
    assert!(!project.to_toml().unwrap().contains("thickness"));

    blade.thickness_hub = vec![[0.0, 6.0], [1.0, 3.0]];
    assert!(!blade.is_default());
    blade.thickness_hub.clear();
    blade.thickness_shroud = vec![[0.0, 6.0], [1.0, 3.0]];
    assert!(!blade.is_default());
    blade.thickness_shroud.clear();
    blade.thickness_same = false;
    assert!(!blade.is_default());
    // Apagar «cubo = tapa» sin nodos sí se guarda: es una elección del usuario.
    project.blade = Some(blade.clone());
    let saved = project.to_toml().unwrap();
    assert!(saved.contains("thickness_same = false"), "{saved}");
    assert_eq!(Project::from_toml(&saved).unwrap().blade, Some(blade));
}

#[test]
fn hand_written_thickness_is_checked() {
    let cases: [(&str, &str); 14] = [
        (
            "thickness_hub = [[0.0, 6.0], [0.5, 6.0], [0.3, 6.0], [1.0, 3.0]]",
            "thickness_hub",
        ),
        (
            "thickness_hub = [[0.0, 6.0], [0.5, 6.0], [0.5, 5.0], [1.0, 3.0]]",
            "thickness_hub",
        ),
        (
            "thickness_hub = [[0.0, 6.0], [0.1, 6.0], [0.2, 6.0], [0.3, 6.0], [0.4, 6.0], [0.5, 6.0], [0.6, 6.0], [0.7, 6.0], [1.0, 3.0]]",
            "thickness_hub",
        ),
        ("thickness_hub = [[0.0, 6.0]]", "thickness_hub"),
        (
            "thickness_hub = []\nthickness_same = false\nthickness_shroud = [[0.0, 6.0], [1.0, 3.0]]",
            "thickness_shroud",
        ),
        (
            "thickness_hub = [[0.0, 6.0], [0.5, 2.9], [1.0, 3.0]]",
            "thickness_hub",
        ),
        ("thickness_hub = [[0.0, 2.9], [1.0, 3.0]]", "thickness_hub"),
        ("thickness_hub = [[0.0, nan], [1.0, 3.0]]", "thickness_hub"),
        (
            "thickness_hub = [[0.0, 6.0], [0.9, inf], [1.0, 3.0]]",
            "thickness_hub",
        ),
        ("thickness_hub = [[0.1, 6.0], [1.0, 3.0]]", "thickness_hub"),
        ("thickness_hub = [[0.0, 6.0], [0.9, 6.0]]", "thickness_hub"),
        (
            "thickness_hub = [[0.0, 6.0, 1.0], [1.0, 3.0]]",
            "thickness_hub",
        ),
        (
            "thickness_hub = [[0.0, 6.0], [1.0, 3.0]]\nthickness_shroud = [[0.0, 6.0], [1.0, 3.0]]",
            "thickness_same",
        ),
        (
            "thickness_same = false\nthickness_hub = [[0.0, 6.0], [1.0, 3.0]]",
            "thickness_shroud",
        ),
    ];
    for (text, field) in cases {
        let error = Project::from_toml(&with_blade(text))
            .expect_err(text)
            .to_string();
        assert!(error.contains(field), "{text}: {error}");
    }
}

#[test]
fn the_exit_node_may_be_1_5_mm() {
    let project =
        Project::from_toml(&with_blade("thickness_hub = [[0.0, 6.0], [1.0, 1.5]]")).unwrap();
    assert_eq!(
        project.blade.unwrap().thickness_hub,
        vec![[0.0, 6.0], [1.0, 1.5]]
    );
    // Solo en x = 1: un nodo interior con 1,5 mm no vale.
    assert!(
        Project::from_toml(&with_blade(
            "thickness_hub = [[0.0, 6.0], [0.5, 1.5], [1.0, 3.0]]"
        ))
        .is_err()
    );
    assert!(Project::from_toml(&with_blade("thickness_hub = [[0.0, 6.0], [1.0, 1.4]]")).is_err());
}
