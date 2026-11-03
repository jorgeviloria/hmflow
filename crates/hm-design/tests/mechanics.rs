use hm_design::mechanics::{
    AXIAL_THRUST_FACTOR, CAST_MIN_THICKNESS, CAST_MIN_THICKNESS_RATIO, DEFAULT_SHAFT_SHEAR_STRESS,
    HUB_RATIO, RADIAL_DESIGN_COEFFICIENT, driver_margin, mechanics, mechanics_diagnostics,
    motor_rating,
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
        nu: None,
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
    let b2tot = result.b2.si + 2.0 * mech.shroud_thickness;
    assert_eq!(mech.radial_width, b2tot);
    let base = input.rho.si * 9.806_65 * result.d2.si * b2tot;
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
    // nq ≈ 9,5, por debajo de la fig. 9.46 (10..95). Con el ηh de Gülich (v0.7)
    // la selección por defecto no alcanza la altura con nq < 10, así que se
    // fijan ψ y Z: lo que se prueba es la mecánica, no la selección.
    let input = input("18 m3/h", "60 m", "2900 rpm");
    let result = hm_design::design_with_options(
        &input,
        &hm_design::DesignOptions {
            psi: Some(0.8),
            blade_count: Some(10),
            // Ojo pequeño: velocidad de entrada alta.
            c0: Some(6.0),
            ..hm_design::DesignOptions::default()
        },
    )
    .unwrap();
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

#[test]
fn blade_and_shroud_thickness_follow_gulich_table_14_3() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let z = f64::from(result.selection.blade_count);
    let ratio = 0.015
        * (7.0 / z).powf(0.4)
        * (1.0 + 0.25 * (result.u2.si / 100.0).powi(2))
        * (1.0 + 0.5 * (result.nq / 100.0).powi(2));
    close(mech.blade_thickness_ratio, ratio, 1e-12);
    let d2 = result.d2.si;
    let expected = (ratio * d2)
        .max(CAST_MIN_THICKNESS_RATIO * d2)
        .max(CAST_MIN_THICKNESS);
    close(mech.blade_thickness, expected, 1e-12);
    close(mech.trailing_edge_thickness, expected / 2.0, 1e-12);
    close(mech.shroud_thickness, 1.25 * expected, 1e-12);
}

#[test]
fn small_impellers_keep_the_casting_minimum() {
    let input = input("10 m3/h", "20 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert!(mech.blade_thickness_ratio * result.d2.si < CAST_MIN_THICKNESS);
    assert_eq!(mech.blade_thickness, CAST_MIN_THICKNESS);
}

#[test]
fn seal_clearance_follows_gulich_eq_3_12() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let dsp = result.d1.si;
    close(
        mech.seal_clearance,
        0.004 * dsp * (0.1 / dsp).powf(0.53),
        1e-12,
    );
}

#[test]
fn driver_margin_follows_api_610() {
    assert_eq!(driver_margin(5e3), 1.25);
    assert_eq!(driver_margin(22e3), 1.25);
    assert_eq!(driver_margin(22.1e3), 1.15);
    assert_eq!(driver_margin(55e3), 1.15);
    assert_eq!(driver_margin(56e3), 1.10);
    assert_eq!(driver_margin(5e6), 1.10);
}

#[test]
fn motor_is_the_next_iec_rating() {
    assert_eq!(motor_rating(22_490.0), Some(30_000.0));
    assert_eq!(motor_rating(30_000.0), Some(30_000.0));
    assert_eq!(motor_rating(1.0), Some(60.0));
    assert_eq!(motor_rating(1_000_001.0), None);
}

#[test]
fn default_pump_gets_a_30_kw_motor() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert_eq!(mech.rated_power, result.p_shaft.si);
    assert_eq!(mech.driver_margin, 1.25);
    close(mech.driver_power, 1.25 * result.p_shaft.si, 1e-12);
    assert_eq!(mech.motor_rating, Some(30_000.0));
    assert!(mech.curve_max_power >= mech.rated_power);
}

#[test]
fn an_overloaded_motor_is_flagged() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let mut mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    mech.curve_max_power = 31_000.0;
    assert!(codes(&mechanics_diagnostics(&mech, &result)).contains(&"HM-DES-029".to_string()));
}

#[test]
fn a_motor_beyond_the_iec_series_is_flagged() {
    let input = input("5000 m3/h", "100 m", "1450 rpm");
    let result = design(&input).unwrap();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert_eq!(mech.motor_rating, None);
    assert!(codes(&mechanics_diagnostics(&mech, &result)).contains(&"HM-DES-030".to_string()));
}
