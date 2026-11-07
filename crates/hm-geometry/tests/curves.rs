use std::f64::consts::{PI, TAU};

use hm_geometry::{Arc, CubicBezier, Curve, GeometryError, Line, Point, Vector};

fn close(left: Point, right: Point) {
    let distance = left.distance(right);
    assert!(
        distance < 1e-12,
        "{left:?} frente a {right:?}, distancia {distance}"
    );
}

#[test]
fn arc_length_is_radius_times_angle() {
    let radius = 2.0;
    let sweep = 1.2;
    let arc = Arc::new(Point::new(0.0, 0.0), radius, 0.3, sweep).expect("arco");
    let length = arc.length();
    assert!((length - radius * sweep).abs() < 1e-12, "{length}");
}

#[test]
fn arc_ends_are_exact_and_the_tangent_is_perpendicular_to_the_radius() {
    let center = Point::new(1.0, -2.0);
    let radius = 0.5;
    let arc = Arc::new(center, radius, 0.0, PI / 2.0).expect("cuarto de círculo");

    close(
        arc.point_at(0.0).expect("inicio"),
        Point::new(center.x + radius, center.y),
    );
    close(
        arc.point_at(1.0).expect("fin"),
        Point::new(center.x, center.y + radius),
    );

    for t in [0.0, 0.25, 0.5, 1.0] {
        let point = arc.point_at(t).expect("punto");
        let radius_vector = Vector::new(point.x - center.x, point.y - center.y);
        let tangent = arc.tangent_at(t).expect("tangente");
        assert!(
            tangent.dot(radius_vector).abs() < 1e-12,
            "t={t}, producto {}",
            tangent.dot(radius_vector)
        );
        assert!((tangent.norm() - 1.0).abs() < 1e-12);
    }

    let full = Arc::new(center, radius, 0.0, TAU).expect("círculo");
    assert!((full.length() - radius * TAU).abs() < 1e-12);
    close(
        full.point_at(0.0).expect("cierre"),
        full.point_at(1.0).expect("cierre"),
    );
}

#[test]
fn a_bezier_that_degenerates_to_a_line_matches_that_line() {
    let start = Point::new(0.0, 0.0);
    let end = Point::new(3.0, 4.0);
    let line = Line::new(start, end).expect("recta");
    let bezier = CubicBezier::new(
        start,
        start + (end - start) * (1.0 / 3.0),
        start + (end - start) * (2.0 / 3.0),
        end,
    )
    .expect("bezier recta");

    assert!((bezier.length() - line.length()).abs() < 1e-12);
    assert!((line.length() - 5.0).abs() < 1e-12);
    close(bezier.point_at(0.0).expect("inicio"), start);
    close(bezier.point_at(1.0).expect("fin"), end);

    let on_line = line.sample(5).expect("muestras de la recta");
    let on_bezier = bezier.sample(5).expect("muestras de la bezier");
    for (left, right) in on_line.iter().zip(&on_bezier) {
        close(*left, *right);
    }
}

#[test]
fn line_ends_are_exact() {
    let start = Point::new(-2.0, 4.0);
    let end = Point::new(8.0, 1.0);
    let line = Line::new(start, end).expect("recta");
    assert_eq!(line.point_at(0.0).expect("inicio"), start);
    assert_eq!(line.point_at(1.0).expect("fin"), end);
    assert_eq!(line.sample(2).expect("extremos"), vec![start, end]);
}

#[test]
fn a_parameter_outside_the_unit_interval_is_rejected() {
    let line = Line::new(Point::new(0.0, 0.0), Point::new(1.0, 0.0)).expect("recta");
    assert_eq!(line.point_at(-0.01).unwrap_err(), GeometryError::OutOfRange);
    assert_eq!(line.point_at(1.01).unwrap_err(), GeometryError::OutOfRange);
    assert_eq!(
        line.point_at(f64::NAN).unwrap_err(),
        GeometryError::OutOfRange
    );
}

#[test]
fn a_point_sized_curve_is_rejected() {
    let point = Point::new(1.0, 1.0);
    assert_eq!(
        Line::new(point, point).unwrap_err(),
        GeometryError::Degenerate
    );
    assert_eq!(
        Arc::new(point, 0.0, 0.0, 1.0).unwrap_err(),
        GeometryError::Degenerate
    );
    assert_eq!(
        Arc::new(point, 1.0, 0.0, 0.0).unwrap_err(),
        GeometryError::Degenerate
    );
    assert_eq!(
        CubicBezier::new(point, point, point, point).unwrap_err(),
        GeometryError::Degenerate
    );
}

#[test]
fn sampling_needs_both_ends() {
    let line = Line::new(Point::new(0.0, 0.0), Point::new(1.0, 0.0)).expect("recta");
    assert_eq!(line.sample(1).unwrap_err(), GeometryError::TooFewSamples);
}

#[test]
fn a_clockwise_arc_has_positive_length_and_turns_the_other_way() {
    let center = Point::new(0.0, 0.0);
    let arc = Arc::new(center, 2.0, PI / 2.0, -PI / 2.0).expect("arco horario");
    assert!((arc.length() - PI).abs() < 1e-12, "{}", arc.length());

    // Sale de (0, 2) hacia +x y llega a (2, 0) bajando.
    close(arc.point_at(0.0).expect("inicio"), Point::new(0.0, 2.0));
    close(arc.point_at(1.0).expect("fin"), Point::new(2.0, 0.0));
    let start = arc.tangent_at(0.0).expect("tangente inicial");
    let end = arc.tangent_at(1.0).expect("tangente final");
    assert!(
        (start.x - 1.0).abs() < 1e-12 && start.y.abs() < 1e-12,
        "{start:?}"
    );
    assert!(
        end.x.abs() < 1e-12 && (end.y + 1.0).abs() < 1e-12,
        "{end:?}"
    );

    // La tangente sigue el recorrido: un paso pequeño en t avanza hacia ella.
    let step = arc.point_at(1e-6).expect("paso") - arc.point_at(0.0).expect("inicio");
    assert!(step.dot(start) > 0.0);
}

#[test]
fn a_curve_with_coordinates_that_are_not_numbers_is_rejected() {
    let good = Point::new(0.0, 0.0);
    for bad in [Point::new(f64::NAN, 0.0), Point::new(0.0, f64::INFINITY)] {
        assert_eq!(Line::new(good, bad).unwrap_err(), GeometryError::Degenerate);
        assert_eq!(Line::new(bad, good).unwrap_err(), GeometryError::Degenerate);
        assert_eq!(
            Arc::new(bad, 1.0, 0.0, 1.0).unwrap_err(),
            GeometryError::Degenerate
        );
    }
}
