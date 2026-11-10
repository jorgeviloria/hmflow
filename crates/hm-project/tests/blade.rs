//! Sección `[blade]`: la forma del álabe y el número de líneas de corriente
//! (esquema 5; las manijas, esquema 6).

use hm_project::{
    BLADE_BOW_RANGE_DEG, BLADE_SHAPES, Blade, DEFAULT_BLADE_SHAPE, DEFAULT_STREAMLINES,
    MIGRATABLE_SCHEMAS, Project, PumpType, SCHEMA_VERSION, STREAMLINE_COUNTS,
};

const V4: &str = include_str!("fixtures/end_suction_v4.hmf");
const V5: &str = include_str!("fixtures/end_suction_v5.hmf");

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
    assert!(saved.starts_with("schema = 6\n"), "{saved}");
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
fn the_schema_is_six_and_migrates_one_to_five() {
    assert_eq!(SCHEMA_VERSION, 6);
    assert_eq!(MIGRATABLE_SCHEMAS, [1, 2, 3, 4, 5]);
    assert_eq!(BLADE_BOW_RANGE_DEG, (-30.0, 30.0));
    assert_eq!(STREAMLINE_COUNTS, [3, 5, 7, 9]);
    assert_eq!(DEFAULT_STREAMLINES, 5);
    assert_eq!(DEFAULT_BLADE_SHAPE, "free_2d");
}

#[test]
fn a_v5_file_loads_with_zero_handles_and_saves_as_v6() {
    assert!(V5.starts_with("schema = 5\n"), "la fixture debe ser v5");
    let project = Project::from_toml(V5).unwrap();
    assert_eq!(project.schema, 6);
    let blade = project.blade.as_ref().unwrap();
    assert_eq!(blade.bow_hub, [0.0, 0.0]);
    assert_eq!(blade.bow_shroud, [0.0, 0.0]);
    assert_eq!(blade.streamlines, 7);
    let saved = project.to_toml().unwrap();
    assert!(saved.starts_with("schema = 6\n"), "{saved}");
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
