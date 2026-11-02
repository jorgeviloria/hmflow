use hm_design::{DesignInput, design, performance_curve};
use hm_units::parse_quantity;

fn input() -> DesignInput {
    DesignInput {
        q: parse_quantity("0.016 m3/s").unwrap(),
        h: parse_quantity("13.8 m").unwrap(),
        n: parse_quantity("1800 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: None,
    }
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

#[test]
fn design_point_is_on_curve() {
    let input = input();
    let result = design(&input).unwrap();
    let curve = performance_curve(&input, &result, 11);

    assert_eq!(curve.len(), 11);
    let point = &curve[7];

    close(point.q.si, input.q.si, 1e-9);
    close(point.h.si, input.h.si, 1e-6);
    close(point.eta, result.eta, 1e-9);
    close(point.p_shaft.si, result.p_shaft.si, 0.1);
    close(point.npshr.si, result.npshr_design.si, 1e-9);
}

#[test]
fn endpoints_span_the_design_range() {
    let input = input();
    let result = design(&input).unwrap();
    let curve = performance_curve(&input, &result, 11);

    close(curve[0].q.si / input.q.si, 0.3, 1e-9);
    close(curve[10].q.si / input.q.si, 1.3, 1e-9);
    assert!(curve[0].h.si > input.h.si);
    assert!(curve[10].h.si < input.h.si);
}

#[test]
fn head_falls_with_flow() {
    let input = input();
    let result = design(&input).unwrap();
    let curve = performance_curve(&input, &result, 11);

    for pair in curve.windows(2) {
        assert!(pair[0].h.si > pair[1].h.si);
    }
}

#[test]
fn npshr_is_minimum_at_design() {
    let input = input();
    let result = design(&input).unwrap();
    let curve = performance_curve(&input, &result, 11);
    let design_point = curve[7].npshr.si;

    assert!(curve[0].npshr.si > design_point);
    assert!(curve[10].npshr.si > design_point);

    for pair in curve[..=7].windows(2) {
        assert!(pair[0].npshr.si > pair[1].npshr.si);
    }
    for pair in curve[7..].windows(2) {
        assert!(pair[0].npshr.si < pair[1].npshr.si);
    }
}

#[test]
fn efficiency_peaks_at_design() {
    let input = input();
    let result = design(&input).unwrap();
    let curve = performance_curve(&input, &result, 11);

    let peak = curve.iter().map(|point| point.eta).fold(f64::MIN, f64::max);
    close(peak, result.eta, 1e-9);
}

#[test]
fn curve_points_are_capped() {
    let input = input();
    let result = design(&input).unwrap();

    assert_eq!(performance_curve(&input, &result, 1).len(), 2);
    assert_eq!(performance_curve(&input, &result, 10_000).len(), 1000);
}

#[test]
fn power_is_positive() {
    let input = input();
    let result = design(&input).unwrap();
    let curve = performance_curve(&input, &result, 11);

    assert!(curve.iter().all(|point| point.p_shaft.si > 0.0));
}
