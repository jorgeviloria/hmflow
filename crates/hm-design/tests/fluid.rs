use hm_design::fluid::{
    water_density, water_kinematic_viscosity, water_vapor_pressure, water_viscosity,
};
use hm_design::{DesignInput, design, viscous_correction, viscous_diagnostics};
use hm_units::parse_quantity;

fn relative(actual: f64, expected: f64) -> f64 {
    (actual / expected - 1.0).abs()
}

#[test]
fn vapor_pressure_matches_iapws_if97_verification() {
    // IAPWS R7-97(2012), tabla 35.
    for (temperature, expected_mpa) in [
        (300.0, 0.353_658_941e-2),
        (500.0, 0.263_889_776e1),
        (600.0, 0.123_443_146e2),
    ] {
        let pressure = water_vapor_pressure(temperature).unwrap();
        assert!(
            relative(pressure, expected_mpa * 1e6) < 1e-8,
            "T = {temperature} K: {pressure} Pa"
        );
    }
    // 20 °C y 100 °C: 2339 Pa y la presión atmosférica normal.
    assert!((water_vapor_pressure(293.15).unwrap() - 2339.2).abs() < 1.0);
    assert!((water_vapor_pressure(373.124).unwrap() - 101_325.0).abs() < 50.0);
    assert!(water_vapor_pressure(270.0).is_none());
    assert!(water_vapor_pressure(700.0).is_none());
}

#[test]
fn viscosity_matches_iapws_2008_verification() {
    // IAPWS R12-08, tabla 4 (sin realce crítico; μ2 = 1 en estos puntos).
    for (temperature, density, expected_micro) in [
        (298.15, 998.0, 889.735100),
        (298.15, 1200.0, 1437.649467),
        (373.15, 1000.0, 307.883622),
        (433.15, 1.0, 14.538324),
        (433.15, 1000.0, 217.685358),
        (873.15, 1.0, 32.619287),
        (873.15, 100.0, 35.802262),
        (873.15, 600.0, 77.430195),
        (1173.15, 1.0, 44.217245),
        (1173.15, 100.0, 47.640433),
        (1173.15, 400.0, 64.154608),
    ] {
        let viscosity = water_viscosity(temperature, density).unwrap();
        assert!(
            relative(viscosity, expected_micro * 1e-6) < 1e-7,
            "T = {temperature} K, ρ = {density}: {viscosity} Pa·s"
        );
    }
    assert!(water_viscosity(-1.0, 998.0).is_none());
}

#[test]
fn kinematic_viscosity_of_water() {
    // ≈ 1,004 cSt a 20 °C y ≈ 0,365 cSt a 80 °C.
    let cold = water_kinematic_viscosity(293.15).unwrap();
    let hot = water_kinematic_viscosity(353.15).unwrap();
    assert!((cold * 1e6 - 1.004).abs() < 0.005, "{cold}");
    assert!((hot * 1e6 - 0.365).abs() < 0.005, "{hot}");
    assert_eq!(water_density(500.0), None);
    assert_eq!(water_kinematic_viscosity(500.0), None);
}

fn input() -> DesignInput {
    DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
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
fn water_viscosity_has_no_practical_effect() {
    let input = input();
    let result = design(&input).unwrap();
    let nu = water_kinematic_viscosity(293.15).unwrap();
    let correction = viscous_correction(&input, &result, nu).unwrap();
    assert!(
        correction.f_h > 0.999 && correction.f_eta > 0.99,
        "{correction:?}"
    );
    assert!(viscous_diagnostics(&correction, result.nq).is_empty());
}

#[test]
fn oil_lowers_head_and_efficiency() {
    let input = input();
    let result = design(&input).unwrap();
    let correction = viscous_correction(&input, &result, 100e-6).unwrap();
    assert!(
        correction.f_h < 0.97 && correction.f_eta < 0.85,
        "{correction:?}"
    );
    assert_eq!(correction.f_q, correction.f_h);
    let expected_power =
        input.rho.si * 9.80665 * correction.flow.si * correction.head.si / correction.efficiency;
    assert!(relative(correction.shaft_power.si, expected_power) < 1e-12);
    assert_eq!(
        codes(&viscous_diagnostics(&correction, result.nq)),
        ["HM-DES-020"]
    );
}

#[test]
fn outside_the_method_range_is_reported() {
    let input = input();
    let result = design(&input).unwrap();
    let thick = viscous_correction(&input, &result, 5000e-6).unwrap();
    assert!(codes(&viscous_diagnostics(&thick, result.nq)).contains(&"HM-DES-021".to_string()));
    let oil = viscous_correction(&input, &result, 20e-6).unwrap();
    assert!(codes(&viscous_diagnostics(&oil, 60.0)).contains(&"HM-DES-021".to_string()));
    // Con agua, nq fuera del rango no importa: la corrección vale 1.
    let water = viscous_correction(&input, &result, 1e-6).unwrap();
    assert!(viscous_diagnostics(&water, 60.0).is_empty());
    assert!(viscous_correction(&input, &result, 0.0).is_err());
}

#[test]
fn reference_water_matches_iapws_at_20_c() {
    let nu = water_kinematic_viscosity(293.15).unwrap();
    assert!(relative(hm_design::REFERENCE_WATER_NU, nu) < 1e-4, "{nu}");
}

#[test]
fn hot_water_gets_no_credit() {
    let input = input();
    let result = design(&input).unwrap();
    let hot = water_kinematic_viscosity(353.15).unwrap();
    let correction = viscous_correction(&input, &result, hot).unwrap();
    assert_eq!(correction.f_h, 1.0);
    assert_eq!(correction.f_eta, 1.0);
}
