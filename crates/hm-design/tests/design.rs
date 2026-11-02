use hm_design::{
    DesignInput, DesignOptions, PARAM_SPECS, design, design_with_options, designed_si, param_spec,
    stored_si_matches, water_density,
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

/// Sin NPSHa, el único diagnóstico admitido es el aviso de que no se evaluó.
fn assert_only_missing_npsha(result: &hm_design::DesignResult) {
    let codes: Vec<&str> = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert_eq!(codes, ["HM-DES-012"]);
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
    assert_only_missing_npsha(&result);
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
    assert_only_missing_npsha(&result);
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
fn sample_duty_with_ten_metres_of_npsha_has_no_npsh_findings() {
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("10 m").unwrap());
    let result = design(&input).unwrap();
    assert!(!result.diagnostics.has_errors());
    assert!(has(&result, "HM-DES-006"));
    assert!(!has(&result, "HM-DES-011"));
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

fn has(result: &hm_design::DesignResult, code: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == code)
}

#[test]
fn npsh_margin_is_enforced_inside_the_preferred_region() {
    // NPSHr_d = 5,14 m; en 1,2 Qd sube a 1,44·5,14 = 7,40 m.
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("7.5 m").unwrap());
    let result = design(&input).unwrap();
    assert!(has(&result, "HM-DES-007"));
    assert!(result.diagnostics.has_errors());
}

#[test]
fn npsh_shortfall_outside_the_preferred_region_only_warns() {
    // 8 m cubre la región 0,7..1,2 Qd pero no 0,3 Qd (8,9 m de NPSHr).
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("8 m").unwrap());
    let result = design(&input).unwrap();
    assert!(!result.diagnostics.has_errors());
    assert!(has(&result, "HM-DES-006"));
    assert!(has(&result, "HM-DES-011"));
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
    assert!(has(&result, "HM-DES-006"));
    assert!(!has(&result, "HM-DES-011"));
}

#[test]
fn missing_npsha_is_reported() {
    let result = design(&input("120 m3/h", "45 m", "2900 rpm")).unwrap();
    assert!(has(&result, "HM-DES-012"));
    assert!(!has(&result, "HM-DES-006"));
    assert!(!has(&result, "HM-DES-007"));
}

#[test]
fn blade_angle_and_count_change_slip_and_cu2() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let base = design(&input).unwrap();

    let steeper = design_with_options(
        &input,
        &DesignOptions {
            beta2_rad: Some(35f64.to_radians()),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(steeper.slip < base.slip);
    assert!((steeper.cu2.si - base.cu2.si).abs() > 0.1);

    let fewer = design_with_options(
        &input,
        &DesignOptions {
            blade_count: Some(4),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(fewer.slip < base.slip);
    assert_eq!(designed_si(&fewer, "slip"), Some(fewer.slip));
    assert_eq!(designed_si(&fewer, "cu2"), Some(fewer.cu2.si));
}

#[test]
fn selections_outside_their_domain_are_rejected() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let cases = [
        DesignOptions {
            psi: Some(-1.0),
            ..DesignOptions::default()
        },
        DesignOptions {
            phi: Some(0.0),
            ..DesignOptions::default()
        },
        DesignOptions {
            beta2_rad: Some(90f64.to_radians()),
            ..DesignOptions::default()
        },
        DesignOptions {
            blade_count: Some(0),
            ..DesignOptions::default()
        },
        DesignOptions {
            margin: Some(0.5),
            ..DesignOptions::default()
        },
        DesignOptions {
            k3: Some(1.0),
            ..DesignOptions::default()
        },
        DesignOptions {
            k_eff: Some(0.0),
            ..DesignOptions::default()
        },
        DesignOptions {
            j: Some(f64::NAN),
            ..DesignOptions::default()
        },
    ];
    for options in cases {
        assert!(
            design_with_options(&input, &options).is_err(),
            "se aceptó {options:?}"
        );
    }
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

#[test]
fn water_density_follows_kell() {
    for (celsius, expected) in [
        (0.0, 999.840),
        (4.0, 999.972),
        (20.0, 998.204),
        (80.0, 971.798),
        (100.0, 958.364),
    ] {
        let density = water_density(celsius + 273.15).unwrap();
        close(density, expected, 0.002);
    }
    assert!(water_density(273.15 - 1.0).is_none());
    assert!(water_density(273.15 + 151.0).is_none());
    assert!(water_density(f64::NAN).is_none());
}
