use hm_design::{
    DesignInput, DesignOptions, PARAM_SPECS, design, design_with_options, designed_si, param_spec,
    stored_si_matches,
};
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
fn options_override_selections() {
    let input = input("0.016 m3/s", "13.8 m", "1800 rpm");
    let default = design(&input).unwrap();
    let options = DesignOptions {
        psi: Some(0.5),
        blade_count: Some(9),
        ..DesignOptions::default()
    };
    let custom = design_with_options(&input, &options).unwrap();

    assert!((custom.selection.psi - 0.5).abs() < 1e-12);
    assert_eq!(custom.selection.blade_count, 9);
    assert!((custom.d2.si - default.d2.si).abs() > 1e-3);
}

#[test]
fn schema_formulas_exist() {
    let registry = hm_formula::Registry::all();
    for spec in PARAM_SPECS {
        if let Some(formula) = spec.formula {
            assert!(
                registry.get(formula).is_some(),
                "{} apunta a {formula}",
                spec.name
            );
        }
        assert!(param_spec(spec.name).is_some());
    }
    assert!(param_spec("no_existe").is_none());
}

#[test]
fn loss_breakdown_closes() {
    let result = design(&input("0.016 m3/s", "13.8 m", "1800 rpm")).unwrap();
    let total =
        result.p_hyd.si + result.p_hyd_loss.si + result.p_vol_loss.si + result.p_mech_loss.si;

    close(total, result.p_shaft.si, 0.5);
    assert!(result.p_hyd_loss.si > 0.0);
    assert!(result.p_vol_loss.si > 0.0);
    assert!(result.p_mech_loss.si > 0.0);
}

#[test]
fn sample_duty_with_ten_metres_of_npsha_has_no_errors() {
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("10 m").unwrap());
    let result = design(&input).unwrap();
    assert!(
        !result.diagnostics.has_errors(),
        "el NPSHa de plantilla no cubre la curva"
    );
}

#[test]
fn stored_snapshot_tolerates_rounding_only() {
    let result = design(&input("120 m3/h", "45 m", "2900 rpm")).unwrap();
    let rounded: f64 = format!("{:.4}", result.specific_diameter_mm)
        .parse()
        .unwrap();
    assert!(stored_si_matches(result.specific_diameter_mm, rounded));
    assert!(!stored_si_matches(result.d2.si, 9.0));
    assert!(!stored_si_matches(result.volute.throat_area.si, 0.0018));
    assert_eq!(designed_si(&result, "d2"), Some(result.d2.si));
    assert!(designed_si(&result, "psi").is_none());
}

#[test]
fn npsh_margin_checks_the_whole_curve() {
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("8 m").unwrap());
    let result = design(&input).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-DES-007")
    );
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
            .any(|diagnostic| diagnostic.code == "HM-DES-007" && diagnostic.is_error())
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
fn eta_m_bounds_are_exclusive() {
    let mut input = input("0.016 m3/s", "13.8 m", "1800 rpm");
    input.eta_m = 0.0;
    assert!(design(&input).is_err());

    input.eta_m = 1.0;
    assert!(design(&input).is_err());
}

#[test]
fn zero_flow_is_rejected() {
    assert!(design(&input("0 m3/s", "45 m", "2900 rpm")).is_err());
}

#[test]
fn wrong_dimension_is_rejected() {
    assert!(design(&input("120 m", "45 m", "2900 rpm")).is_err());
}
