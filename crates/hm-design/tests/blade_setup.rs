//! Forma del álabe y número de líneas de corriente.

use hm_design::blade_setup::{
    BladeFamily, BladeSetup, BladeShape, DEFAULT_STREAMLINES, STREAMLINE_COUNTS,
};

#[test]
fn the_default_is_free_2d_with_five_streamlines() {
    assert_eq!(BladeShape::default(), BladeShape::Free2D);
    assert_eq!(
        BladeSetup::default(),
        BladeSetup {
            shape: BladeShape::Free2D,
            streamlines: 5,
        }
    );
    assert_eq!(DEFAULT_STREAMLINES, 5);
    assert_eq!(STREAMLINE_COUNTS, [3, 5, 7, 9]);
    assert!(STREAMLINE_COUNTS.contains(&DEFAULT_STREAMLINES));
}

#[test]
fn only_free_2d_is_available() {
    let available: Vec<BladeShape> = BladeShape::ALL
        .into_iter()
        .filter(|shape| shape.is_available())
        .collect();
    assert_eq!(available, [BladeShape::Free2D]);
}

#[test]
fn each_shape_says_when_it_arrives() {
    use BladeShape::*;
    assert_eq!(Free3D.arrives_in(), Some(2));
    assert_eq!(Ruled3D.arrives_in(), Some(2));
    assert_eq!(Free2D.arrives_in(), None);
    for shape in [Circular2D, Straight2D, CircularFree2D, LogSpiralStraight2D] {
        assert_eq!(shape.arrives_in(), Some(4), "{shape:?}");
    }
    // Disponible y «llega en» son la misma pregunta vista por los dos lados.
    for shape in BladeShape::ALL {
        assert_eq!(
            shape.is_available(),
            shape.arrives_in().is_none(),
            "{shape:?}"
        );
    }
}

#[test]
fn ids_round_trip_and_unknown_ids_are_none() {
    for shape in BladeShape::ALL {
        assert_eq!(BladeShape::from_id(shape.id()), Some(shape));
    }
    assert_eq!(BladeShape::from_id("foo"), None);
    assert_eq!(BladeShape::from_id(""), None);
    assert_eq!(BladeShape::Free2D.id(), "free_2d");
    assert_eq!(
        BladeShape::LogSpiralStraight2D.id(),
        "log_spiral_straight_2d"
    );
}

#[test]
fn the_families_are_two_3d_and_five_2d() {
    let count = |family| {
        BladeShape::ALL
            .into_iter()
            .filter(|shape| shape.family() == family)
            .count()
    };
    assert_eq!(count(BladeFamily::ThreeD), 2);
    assert_eq!(count(BladeFamily::TwoD), 5);
    assert_eq!(BladeFamily::ThreeD.name(), "3D");
    assert_eq!(BladeFamily::TwoD.name(), "2D");
}

#[test]
fn the_names_are_the_ones_of_the_selector() {
    let names: Vec<&str> = BladeShape::ALL.into_iter().map(BladeShape::name).collect();
    assert_eq!(
        names,
        [
            "Libre",
            "Reglada",
            "Libre",
            "Circular",
            "Recta",
            "Circular + libre",
            "Espiral logarítmica + recta",
        ]
    );
}
