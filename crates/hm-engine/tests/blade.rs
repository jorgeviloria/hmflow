//! La sección `[blade]` del proyecto y el `BladeSetup` del diseño.

use hm_design::blade_setup::{self, BladeSetup, BladeShape, CamberBows};
use hm_engine::{blade_section_of, blade_setup_of};
use hm_project::Blade;

#[test]
fn the_file_ids_are_the_shapes_of_the_design_crate() {
    assert_eq!(
        hm_project::BLADE_SHAPES,
        BladeShape::ALL.map(BladeShape::id)
    );
    assert_eq!(
        hm_project::STREAMLINE_COUNTS,
        blade_setup::STREAMLINE_COUNTS
    );
    assert_eq!(
        hm_project::DEFAULT_STREAMLINES,
        blade_setup::DEFAULT_STREAMLINES
    );
    assert_eq!(hm_project::DEFAULT_BLADE_SHAPE, BladeShape::default().id());
}

#[test]
fn no_section_is_the_default_setup_and_the_default_setup_is_no_section() {
    assert_eq!(blade_setup_of(None), Ok(BladeSetup::default()));
    assert_eq!(blade_section_of(&BladeSetup::default()), None);
    assert_eq!(
        blade_setup_of(Some(&Blade::default())),
        Ok(BladeSetup::default())
    );
}

#[test]
fn a_setup_round_trips_through_the_section() {
    for streamlines in [3, 7, 9] {
        let setup = BladeSetup {
            shape: BladeShape::Free2D,
            streamlines,
            ..BladeSetup::default()
        };
        let section = blade_section_of(&setup).expect("no es el de por defecto");
        assert_eq!(section.shape, "free_2d");
        assert_eq!(section.streamlines, streamlines);
        assert_eq!(blade_setup_of(Some(&section)), Ok(setup));
    }
    // Una forma que aún no está disponible también se guarda y se lee.
    let setup = BladeSetup {
        shape: BladeShape::Ruled3D,
        streamlines: 5,
        ..BladeSetup::default()
    };
    let section = blade_section_of(&setup).unwrap();
    assert_eq!(section.shape, "ruled_3d");
    assert_eq!(blade_setup_of(Some(&section)), Ok(setup));
}

#[test]
fn a_hand_built_section_with_bad_values_is_an_error() {
    let shape = blade_setup_of(Some(&Blade {
        shape: "foo".into(),
        streamlines: 5,
        ..Blade::default()
    }))
    .unwrap_err();
    assert!(
        shape.contains("forma de álabe desconocida «foo»"),
        "{shape}"
    );
    let count = blade_setup_of(Some(&Blade {
        shape: "free_2d".into(),
        streamlines: 4,
        ..Blade::default()
    }))
    .unwrap_err();
    assert!(count.contains("debe ser 3, 5, 7 o 9"), "{count}");
}

#[test]
fn handles_cross_the_engine_both_ways() {
    let setup = BladeSetup {
        shape: BladeShape::Free3D,
        streamlines: 7,
        bows: CamberBows {
            hub: (3.5, -2.0),
            shroud: (0.0, 1.25),
        },
        ..BladeSetup::default()
    };
    let section = blade_section_of(&setup).expect("no es el de por defecto");
    assert_eq!(section.bow_hub, [3.5, -2.0]);
    assert_eq!(section.bow_shroud, [0.0, 1.25]);
    assert_eq!(blade_setup_of(Some(&section)), Ok(setup));

    // Solo las manijas ya hacen la elección distinta de la de por defecto,
    // también con la forma 2D (se guardan para cuando se vuelva a 3D).
    let only_bows = BladeSetup {
        bows: CamberBows {
            hub: (1.0, 0.0),
            shroud: (0.0, 0.0),
        },
        ..BladeSetup::default()
    };
    let section = blade_section_of(&only_bows).expect("las manijas se guardan");
    assert_eq!(section.shape, "free_2d");
    assert_eq!(blade_setup_of(Some(&section)), Ok(only_bows));
    assert_eq!(blade_section_of(&BladeSetup::default()), None);
}

#[test]
fn the_handle_range_is_the_same_in_project_and_design() {
    assert_eq!(
        hm_project::BLADE_BOW_RANGE_DEG,
        hm_design::BETA_BOW_RANGE_DEG
    );
}

#[test]
fn a_hand_made_section_out_of_range_is_rejected() {
    for section in [
        Blade {
            bow_hub: [45.0, 0.0],
            ..Blade::default()
        },
        Blade {
            bow_shroud: [0.0, f64::NAN],
            ..Blade::default()
        },
        Blade {
            bow_shroud: [f64::INFINITY, 0.0],
            ..Blade::default()
        },
    ] {
        let error = blade_setup_of(Some(&section)).unwrap_err();
        assert!(error.contains("bow_"), "{error}");
        assert!(error.contains("-30") && error.contains("30"), "{error}");
    }
}
