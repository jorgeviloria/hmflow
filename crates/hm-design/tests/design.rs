use hm_design::{DesignInput, design};
use hm_units::parse_quantity;

fn input(q: &str, h: &str, n: &str) -> DesignInput {
    DesignInput {
        q: parse_quantity(q).unwrap(),
        h: parse_quantity(h).unwrap(),
        n: parse_quantity(n).unwrap(),
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
fn d1a1_reference() {
    let result = design(&input("0.016 m3/s", "13.8 m", "1800 rpm")).unwrap();

    close(result.nq, 31.7997, 0.05);
    close(result.specific_diameter_mm, 93.24, 0.5);
    close(result.selection.psi, 0.87, 1e-12);
    close(result.selection.phi, 0.13, 1e-12);
    close(result.selection.beta2_deg, 23.5, 1e-12);
    assert_eq!(result.selection.blade_count, 7);

    close(result.d2.value(), 0.1872, 0.001);
    close(result.b2.value(), 0.0122, 0.0002);
    close(result.d0.value(), 0.0764, 0.001);
    close(result.d1.value(), 0.0787, 0.001);
    close(result.b1.value(), 0.0297, 0.0005);
    close(result.u2.value(), 17.64, 0.02);
    close(result.cm2.value(), 2.293, 0.02);

    close(result.eta_h, 0.870, 0.002);
    close(result.eta_v, 0.9722, 0.002);
    close(result.eta, 0.812, 0.003);
    close(result.p_hyd.value(), 2161.4, 5.0);
    close(result.p_shaft.value(), 2661.8, 10.0);
    close(result.p_max.value(), 2928.0, 15.0);
    close(result.torque.value(), 14.12, 0.05);
    close(result.torque_max.value(), 15.53, 0.05);
    close(result.npshr_design.value(), 1.67, 0.05);
    close(result.shutoff_head.value(), 19.0, 0.1);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn existing_case_is_consistent() {
    let result = design(&input("120 m3/h", "45 m", "2900 rpm")).unwrap();

    close(result.nq, 30.474, 0.05);
    close(result.d2.value(), 0.2097, 0.001);
    close(result.b2.value(), 0.0126, 0.0003);
    close(result.d1.value(), 0.1135, 0.001);
    close(result.b1.value(), 0.0428, 0.001);
    close(result.eta, 0.816, 0.003);
    close(result.p_hyd.value(), 14681.5, 5.0);
    close(result.p_shaft.value(), 17990.0, 50.0);
    close(result.torque.value(), 59.25, 0.2);
    close(result.npshr_design.value(), 5.14, 0.1);
    close(result.shutoff_head.value(), 61.7, 0.2);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn npsh_margin_is_checked() {
    let mut input = input("0.016 m3/s", "13.8 m", "1800 rpm");
    input.npsha = Some(parse_quantity("1 m").unwrap());
    let result = design(&input).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-DES-007")
    );

    input.npsha = Some(parse_quantity("6 m").unwrap());
    let result = design(&input).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-DES-006")
    );
}

#[test]
fn zero_flow_is_rejected() {
    assert!(design(&input("0 m3/s", "45 m", "2900 rpm")).is_err());
}

#[test]
fn wrong_dimension_is_rejected() {
    assert!(design(&input("120 m", "45 m", "2900 rpm")).is_err());
}
