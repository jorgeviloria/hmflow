use hm_design::mechanics::{
    AXIAL_THRUST_FACTOR, DEFAULT_SHAFT_SHEAR_STRESS, HUB_RATIO, RADIAL_DESIGN_COEFFICIENT,
    mechanics, mechanics_diagnostics,
};
use hm_design::{DesignInput, curve_head, design};
use hm_units::parse_quantity;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance * expected.abs(),
        "actual {actual}, esperado {expected}, tolerancia relativa {tolerance}"
    );
}

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

fn codes(diagnostics: &hm_core::Diagnostics) -> Vec<String> {
    diagnostics.iter().map(|d| d.code.clone()).collect()
}

#[test]
fn shaft_comes_from_the_maximum_torque() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let torque = result.torque_max.si;
    assert_eq!(mech.shaft_torque, torque);
    let expected = (16.0 * torque / (std::f64::consts::PI * DEFAULT_SHAFT_SHEAR_STRESS)).cbrt();
    close(mech.shaft_min_diameter, expected, 1e-12);
    close(mech.hub_diameter, HUB_RATIO * expected, 1e-12);
}

#[test]
fn a_lower_allowable_stress_thickens_the_shaft() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let cold = mechanics(&input, &result, 22.5e6).unwrap();
    let hot = mechanics(&input, &result, 10.1e6).unwrap();
    close(
        hot.shaft_min_diameter / cold.shaft_min_diameter,
        (22.5f64 / 10.1).cbrt(),
        1e-12,
    );
}

#[test]
fn radial_thrust_follows_gulich_table_9_7() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let base = input.rho.si * 9.806_65 * result.d2.si * result.b2.si;
    close(
        mech.radial_thrust_design,
        RADIAL_DESIGN_COEFFICIENT * base * input.h.si,
        1e-12,
    );
    close(
        mech.radial_thrust_shutoff,
        mech.radial_coefficient_shutoff * base * result.shutoff_head.si,
        1e-12,
    );
    close(
        mech.radial_thrust_overload,
        RADIAL_DESIGN_COEFFICIENT * 1.44 * base * curve_head(&input, &result, 1.2),
        1e-12,
    );
    assert!(mech.radial_thrust_shutoff > mech.radial_thrust_overload);
    assert!(mech.radial_thrust_overload > mech.radial_thrust_design);
}

#[test]
fn axial_thrust_is_bracketed_by_the_factor_range() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let (low, high) = mech.axial_thrust;
    assert!(low > 0.0);
    close(
        high / low,
        AXIAL_THRUST_FACTOR.1 / AXIAL_THRUST_FACTOR.0,
        1e-12,
    );
    assert_eq!(mech.front_seal_diameter, result.d1.si);
    assert_eq!(mech.shaft_seal_diameter, mech.hub_diameter);
}

#[test]
fn a_normal_pump_has_no_mechanical_warnings() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert!(mechanics_diagnostics(&mech, &result).is_empty());
}

#[test]
fn a_thick_hub_in_a_small_eye_is_flagged() {
    let input = input("10 m3/h", "120 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let found = codes(&mechanics_diagnostics(&mech, &result));
    assert!(found.contains(&"HM-DES-027".to_string()), "{found:?}");
    assert!(found.contains(&"HM-DES-028".to_string()), "{found:?}");
}

#[test]
fn shutoff_coefficient_is_held_at_the_edge_of_the_figure() {
    let input = input("1500 m3/h", "20 m", "1450 rpm");
    let result = design(&input).unwrap();
    assert!(result.nq > 95.0);
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    close(mech.radial_coefficient_shutoff, 0.292_922_562_5, 1e-9);
}

#[test]
fn a_non_positive_stress_is_rejected() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    assert!(mechanics(&input, &result, 0.0).is_err());
    assert!(mechanics(&input, &result, f64::NAN).is_err());
}
