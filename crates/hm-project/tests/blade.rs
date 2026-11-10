//! Sección `[blade]`: la forma del álabe y el número de líneas de corriente
//! (esquema 5).

use hm_project::{
    BLADE_SHAPES, Blade, DEFAULT_BLADE_SHAPE, DEFAULT_STREAMLINES, MIGRATABLE_SCHEMAS, Project,
    PumpType, SCHEMA_VERSION, STREAMLINE_COUNTS,
};

const V4: &str = include_str!("fixtures/end_suction_v4.hmf");

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
fn a_v4_file_without_blade_loads_as_default_and_saves_as_v5_without_the_section() {
    assert!(V4.starts_with("schema = 4\n"), "la fixture debe ser v4");
    let project = Project::from_toml(V4).unwrap();
    assert_eq!(project.schema, 5);
    assert_eq!(project.blade, None);
    let saved = project.to_toml().unwrap();
    assert!(saved.starts_with("schema = 5\n"), "{saved}");
    assert!(!saved.contains("[blade]"), "{saved}");
}

#[test]
fn a_non_default_blade_round_trips() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    let blade = Blade {
        shape: "free_2d".into(),
        streamlines: 9,
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
fn the_schema_is_five_and_migrates_one_to_four() {
    assert_eq!(SCHEMA_VERSION, 5);
    assert_eq!(MIGRATABLE_SCHEMAS, [1, 2, 3, 4]);
    assert_eq!(STREAMLINE_COUNTS, [3, 5, 7, 9]);
    assert_eq!(DEFAULT_STREAMLINES, 5);
    assert_eq!(DEFAULT_BLADE_SHAPE, "free_2d");
}
