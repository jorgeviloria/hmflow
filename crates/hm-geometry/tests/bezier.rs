//! Bézier de cualquier grado: n + 1 puntos de control dan grado n.

use hm_geometry::{Bezier, Curve, GeometryError, Line, Point};

fn close(left: Point, right: Point, tolerance: f64) {
    let distance = left.distance(right);
    assert!(distance < tolerance, "{left:?} frente a {right:?}");
}

fn quartic() -> Bezier {
    Bezier::new(vec![
        Point::new(0.0, 0.0),
        Point::new(1.0, 2.0),
        Point::new(3.0, 3.0),
        Point::new(5.0, 1.0),
        Point::new(6.0, -1.0),
    ])
    .expect("cuártica")
}

/// Forma de Bernstein, escrita aparte de De Casteljau para contrastarlo.
fn bernstein(points: &[Point], t: f64) -> Point {
    let n = points.len() - 1;
    let mut x = 0.0;
    let mut y = 0.0;
    for (i, point) in points.iter().enumerate() {
        let mut binomial = 1.0;
        for k in 0..i {
            binomial = binomial * (n - k) as f64 / (k + 1) as f64;
        }
        let weight = binomial * t.powi(i as i32) * (1.0 - t).powi((n - i) as i32);
        x += weight * point.x;
        y += weight * point.y;
    }
    Point::new(x, y)
}

#[test]
fn five_points_make_a_quartic() {
    assert_eq!(quartic().degree(), 4);
    assert_eq!(quartic().points().len(), 5);
}

#[test]
fn de_casteljau_agrees_with_the_bernstein_form() {
    let curve = quartic();
    for step in 0..=20 {
        let t = f64::from(step) / 20.0;
        close(
            curve.point_at(t).expect("punto"),
            bernstein(curve.points(), t),
            1e-12,
        );
    }
}

#[test]
fn the_ends_are_the_first_and_last_points_exactly() {
    let curve = quartic();
    assert_eq!(curve.point_at(0.0).expect("inicio"), Point::new(0.0, 0.0));
    assert_eq!(curve.point_at(1.0).expect("fin"), Point::new(6.0, -1.0));
}

#[test]
fn the_last_point_survives_rounding() {
    // 0,1 + (0,001 − 0,1) no da 0,001 en coma flotante.
    let end = Point::new(0.001, 0.001);
    let curve = Bezier::new(vec![Point::new(0.1, 0.1), end]).expect("segmento");
    assert_eq!(curve.point_at(1.0).expect("fin"), end);
    assert_eq!(*curve.sample(3).expect("muestras").last().unwrap(), end);
}

#[test]
fn the_end_tangents_follow_the_first_and_last_legs() {
    let curve = quartic();
    let start = curve.tangent_at(0.0).expect("tangente inicial");
    let end = curve.tangent_at(1.0).expect("tangente final");
    let first = (Point::new(1.0, 2.0) - Point::new(0.0, 0.0)) * (1.0 / 5f64.sqrt());
    let last = (Point::new(6.0, -1.0) - Point::new(5.0, 1.0)) * (1.0 / 5f64.sqrt());
    assert!((start.x - first.x).abs() < 1e-12 && (start.y - first.y).abs() < 1e-12);
    assert!((end.x - last.x).abs() < 1e-12 && (end.y - last.y).abs() < 1e-12);
}

#[test]
fn the_tangent_matches_a_finite_difference() {
    let curve = quartic();
    for t in [0.2, 0.5, 0.8] {
        let h = 1e-6;
        let step = curve.point_at(t + h).expect("adelante") - curve.point_at(t - h).expect("atrás");
        let direction = step * (1.0 / step.norm());
        let tangent = curve.tangent_at(t).expect("tangente");
        assert!(direction.dot(tangent) > 1.0 - 1e-9, "{t}");
    }
}

#[test]
fn raising_the_degree_does_not_change_the_curve() {
    // Elevación de grado: Q_i = i/(n+1)·P_(i−1) + (1 − i/(n+1))·P_i.
    let cubic = [
        Point::new(0.0, 0.0),
        Point::new(1.0, 3.0),
        Point::new(4.0, 3.0),
        Point::new(5.0, 0.0),
    ];
    let n = 3.0;
    let mut raised = vec![cubic[0]];
    for i in 1..4 {
        let a = i as f64 / (n + 1.0);
        let left = cubic[i - 1];
        let right = cubic[i];
        raised.push(Point::new(
            a * left.x + (1.0 - a) * right.x,
            a * left.y + (1.0 - a) * right.y,
        ));
    }
    raised.push(cubic[3]);
    let low = Bezier::cubic(cubic[0], cubic[1], cubic[2], cubic[3]).expect("cúbica");
    let high = Bezier::new(raised).expect("cuártica");
    assert_eq!(high.degree(), 4);
    for step in 0..=10 {
        let t = f64::from(step) / 10.0;
        close(low.point_at(t).unwrap(), high.point_at(t).unwrap(), 1e-12);
    }
    assert!((low.length() - high.length()).abs() < 1e-9);
}

#[test]
fn evenly_spaced_points_on_a_line_give_that_line() {
    let start = Point::new(0.0, 0.0);
    let end = Point::new(3.0, 4.0);
    let line = Line::new(start, end).expect("recta");
    let points = (0..=4)
        .map(|i| start + (end - start) * (f64::from(i) / 4.0))
        .collect();
    let curve = Bezier::new(points).expect("recta de grado 4");
    assert!((curve.length() - 5.0).abs() < 1e-12, "{}", curve.length());
    for (left, right) in line
        .sample(7)
        .expect("recta")
        .iter()
        .zip(&curve.sample(7).expect("bézier"))
    {
        close(*left, *right, 1e-12);
    }
}

#[test]
fn two_points_are_a_straight_segment() {
    let curve = Bezier::new(vec![Point::new(1.0, 1.0), Point::new(4.0, 5.0)]).expect("segmento");
    assert_eq!(curve.degree(), 1);
    assert!((curve.length() - 5.0).abs() < 1e-12);
}

#[test]
fn a_curve_needs_two_finite_and_distinct_points() {
    let origin = Point::new(0.0, 0.0);
    assert_eq!(
        Bezier::new(vec![origin]).unwrap_err(),
        GeometryError::TooFewSamples
    );
    assert_eq!(
        Bezier::new(vec![]).unwrap_err(),
        GeometryError::TooFewSamples
    );
    assert_eq!(
        Bezier::new(vec![origin, origin, origin]).unwrap_err(),
        GeometryError::Degenerate
    );
    assert_eq!(
        Bezier::new(vec![origin, Point::new(f64::NAN, 1.0)]).unwrap_err(),
        GeometryError::Degenerate
    );
}

#[test]
fn a_parameter_outside_zero_to_one_is_rejected() {
    assert_eq!(
        quartic().point_at(1.5).unwrap_err(),
        GeometryError::OutOfRange
    );
    assert_eq!(
        quartic().tangent_at(-0.1).unwrap_err(),
        GeometryError::OutOfRange
    );
}

#[test]
fn a_straight_bezier_has_no_curvature() {
    let points = (0..=4)
        .map(|i| Point::new(f64::from(i), 2.0 * f64::from(i)))
        .collect();
    let line = Bezier::new(points).expect("recta");
    for t in [0.0, 0.3, 1.0] {
        assert!(line.curvature_at(t).unwrap().abs() < 1e-12);
    }
}

#[test]
fn curvature_matches_finite_differences() {
    let curve = quartic();
    let h = 1e-4;
    for t in [0.2, 0.5, 0.8] {
        let p = |s: f64| curve.point_at(s).unwrap();
        let (a, b, c) = (p(t - h), p(t), p(t + h));
        let (dx, dy) = ((c.x - a.x) / (2.0 * h), (c.y - a.y) / (2.0 * h));
        let (ddx, ddy) = (
            (c.x - 2.0 * b.x + a.x) / (h * h),
            (c.y - 2.0 * b.y + a.y) / (h * h),
        );
        let expected = (dx * ddy - dy * ddx) / (dx * dx + dy * dy).powf(1.5);
        let found = curve.curvature_at(t).unwrap();
        assert!(
            (found - expected).abs() < 1e-5 * expected.abs().max(1.0),
            "{t}: {found} {expected}"
        );
    }
}

#[test]
fn coupled_end_points_give_zero_curvature_at_the_ends() {
    // P1 sobre P0–P2 y P3 sobre P2–P4: la curva arranca y llega sin curvatura.
    let (p0, p2, p4) = (
        Point::new(0.0, 0.0),
        Point::new(10.0, 0.0),
        Point::new(10.0, 10.0),
    );
    let curve = Bezier::new(vec![p0, p0 + (p2 - p0) * 0.6, p2, p4 + (p2 - p4) * 0.6, p4]).unwrap();
    assert!(curve.curvature_at(0.0).unwrap().abs() < 1e-12);
    assert!(curve.curvature_at(1.0).unwrap().abs() < 1e-12);
    assert!(
        curve.curvature_at(0.5).unwrap() > 0.0,
        "gira a la izquierda"
    );
}

#[test]
fn curvature_needs_a_parameter_in_range() {
    assert_eq!(
        quartic().curvature_at(1.2).unwrap_err(),
        GeometryError::OutOfRange
    );
}
