//! La sección `[blade]` del proyecto y el `BladeSetup` del diseño.

use hm_design::blade_setup::{self, BladeSetup, BladeShape};
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
    }))
    .unwrap_err();
    assert!(
        shape.contains("forma de álabe desconocida «foo»"),
        "{shape}"
    );
    let count = blade_setup_of(Some(&Blade {
        shape: "free_2d".into(),
        streamlines: 4,
    }))
    .unwrap_err();
    assert!(count.contains("debe ser 3, 5, 7 o 9"), "{count}");
}
