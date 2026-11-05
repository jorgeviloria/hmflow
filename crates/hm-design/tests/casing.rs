//! Diseño de un rodete para una carcasa existente. Carcasa ficticia.

use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics};
use hm_design::{CasingFit, DesignInput, DesignOptions, design, design_with_options};
use hm_units::parse_quantity;

/// ψ de D2 = 210 mm con 45 m y 2900 rpm, calculado aparte.
const PSI_210: f64 = 0.868_02;

fn input() -> DesignInput {
    let q = |text: &str| parse_quantity(text).unwrap();
    DesignInput {
        q: q("120 m3/h"),
        h: q("45 m"),
        n: q("2900 rpm"),
        rho: q("998.2 kg/m3"),
        eta_m: 0.95,
        npsha: None,
        nu: None,
    }
}

fn sample() -> CasingFit {
    CasingFit {
        d2: 0.210,
        neck_diameter: 0.090,
        eye_diameter_max: 0.075,
        bore_diameter: 0.020,
        hub_diameter: 0.030,
        b3: 0.030,
        cutwater_radius: 0.111,
        channels: 1,
        wrap_deg: 180.0,
        throat_area: None,
    }
}

fn with(casing: CasingFit) -> DesignOptions {
    DesignOptions {
        casing: Some(casing),
        ..DesignOptions::default()
    }
}

#[test]
fn d2_is_the_casing_one_and_psi_follows() {
    let result = design_with_options(&input(), &with(sample())).unwrap();
    assert!((result.d2.si - 0.210).abs() < 1e-12, "{}", result.d2.si);
    assert!((result.selection.psi - PSI_210).abs() < 1e-5);
    let u2 = std::f64::consts::PI * 0.210 * 2900.0 / 60.0;
    assert!((result.u2.si - u2).abs() < 1e-9);
}

#[test]
fn a_casing_with_the_free_d2_changes_no_selection() {
    let free = design(&input()).unwrap();
    let casing = CasingFit {
        d2: free.d2.si,
        ..sample()
    };
    let fitted = design_with_options(&input(), &with(casing)).unwrap();
    assert!((fitted.selection.psi / free.selection.psi - 1.0).abs() < 1e-9);
    assert!((fitted.selection.phi / free.selection.phi - 1.0).abs() < 1e-9);
}

#[test]
fn the_free_design_is_not_the_sample_casing() {
    // Si coincidieran, estas pruebas no distinguirían nada.
    let free = design(&input()).unwrap();
    assert!((free.d2.si / 0.210 - 1.0).abs() > 0.03);
}

#[test]
fn a_manual_psi_that_agrees_is_accepted() {
    let first = design_with_options(&input(), &with(sample())).unwrap();
    let options = DesignOptions {
        psi: Some(first.selection.psi),
        ..with(sample())
    };
    assert!(design_with_options(&input(), &options).is_ok());
}

#[test]
fn a_manual_psi_that_disagrees_names_both() {
    let options = DesignOptions {
        psi: Some(1.1),
        ..with(sample())
    };
    let error = design_with_options(&input(), &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("1.1"), "{error}");
    assert!(error.contains("0.868"), "{error}");
    assert!(error.contains("[casing]"), "{error}");
}

#[test]
fn a_psi_that_is_not_a_number_is_rejected() {
    for psi in [f64::NAN, f64::INFINITY] {
        let options = DesignOptions {
            psi: Some(psi),
            ..with(sample())
        };
        assert!(design_with_options(&input(), &options).is_err(), "{psi}");
    }
}

#[test]
fn an_unreachable_head_blames_the_casing_not_psi() {
    let casing = CasingFit {
        d2: 0.150,
        ..sample()
    };
    let error = design_with_options(&input(), &with(casing))
        .unwrap_err()
        .to_string();
    assert!(error.contains("ningún β2 da la altura"), "{error}");
    assert!(error.contains("D2 = 150.0 mm"), "{error}");
    assert!(!error.contains("reduce ψ"), "{error}");
    assert!(!error.contains("aumenta Z"), "{error}");
}

#[test]
fn without_a_casing_the_advice_stays() {
    let options = DesignOptions {
        psi: Some(1.7),
        ..DesignOptions::default()
    };
    let error = design_with_options(&input(), &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("reduce ψ"), "{error}");
}

#[test]
fn the_default_flow_coefficient_follows_the_casing_psi() {
    // La φ de tabla mantiene la anchura relativa b2/D2 de Gülich aunque D2
    // no sea el libre.
    let free = design(&input()).unwrap();
    let fitted = design_with_options(&input(), &with(sample())).unwrap();
    let relative = |result: &hm_design::DesignResult| result.b2.si / result.d2.si;
    assert!(
        (relative(&fitted) / relative(&free) - 1.0).abs() < 0.01,
        "{} frente a {}",
        relative(&fitted),
        relative(&free)
    );
}

#[test]
fn a_bad_casing_value_is_rejected_by_name() {
    type Edit = fn(&mut CasingFit);
    let cases: [(Edit, &str); 9] = [
        (|c| c.d2 = -0.21, "d2"),
        (|c| c.d2 = f64::NAN, "d2"),
        (|c| c.neck_diameter = 0.0, "neck_diameter"),
        (|c| c.eye_diameter_max = f64::INFINITY, "eye_diameter_max"),
        (|c| c.hub_diameter = -1.0, "hub_diameter"),
        (|c| c.b3 = 0.0, "b3"),
        (|c| c.channels = 3, "channels"),
        (|c| c.wrap_deg = 200.0, "wrap"),
        (|c| c.throat_area = Some(-1.0), "throat_area"),
    ];
    for (edit, field) in cases {
        let mut casing = sample();
        edit(&mut casing);
        let error = design_with_options(&input(), &with(casing))
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!("casing.{field}")), "{error}");
    }
}

fn free_eye() -> f64 {
    design_with_options(&input(), &with_eye_limit(0.180))
        .unwrap()
        .d1
        .si
}

fn with_eye_limit(limit: f64) -> DesignOptions {
    with(CasingFit {
        eye_diameter_max: limit,
        neck_diameter: 0.190,
        ..sample()
    })
}

fn codes(result: &hm_design::DesignResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect()
}

#[test]
fn a_tight_neck_limits_the_eye_and_says_so() {
    let free = free_eye();
    let limit = free * 0.9;
    let result = design_with_options(&input(), &with_eye_limit(limit)).unwrap();
    assert!((result.d1.si - limit).abs() < 1e-9, "{}", result.d1.si);
    let warning = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "HM-DES-039")
        .unwrap();
    assert!(
        warning.message.contains(&format!("{:.1} mm", limit * 1e3)),
        "{}",
        warning.message
    );
    assert!(
        warning.message.contains(&format!("{:.1} mm", free * 1e3)),
        "{}",
        warning.message
    );
}

#[test]
fn a_roomy_neck_leaves_the_best_eye() {
    let free = free_eye();
    let result = design_with_options(&input(), &with_eye_limit(free * 1.1)).unwrap();
    assert!((result.d1.si - free).abs() < 1e-6);
    assert!(!codes(&result).contains(&"HM-DES-039".to_string()));
}

#[test]
fn the_eye_search_uses_the_casing_hub() {
    let thick = CasingFit {
        hub_diameter: 0.050,
        eye_diameter_max: 0.180,
        neck_diameter: 0.190,
        ..sample()
    };
    let result = design_with_options(&input(), &with(thick)).unwrap();
    assert!(result.d1.si > free_eye());
}

#[test]
fn a_manual_eye_over_the_limit_is_an_error_that_names_everything() {
    let options = DesignOptions {
        c0: Some(1.5),
        j: Some(1.1),
        ..with(sample())
    };
    let error = design_with_options(&input(), &options)
        .unwrap_err()
        .to_string();
    for expected in ["c0", "J", "75.0 mm"] {
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn a_manual_eye_inside_the_limit_is_kept() {
    let options = DesignOptions {
        c0: Some(5.0),
        j: Some(1.1),
        ..with_eye_limit(0.110)
    };
    let result = design_with_options(&input(), &options).unwrap();
    assert_eq!(result.selection.c0, 5.0);
}

#[test]
fn the_seal_is_the_neck_with_a_casing_and_the_eye_without() {
    let fitted = design_with_options(&input(), &with(sample())).unwrap();
    let mech = mechanics(&input(), &fitted, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert_eq!(mech.front_seal_diameter, 0.090);

    let wider = CasingFit {
        neck_diameter: 0.110,
        ..sample()
    };
    let wider = design_with_options(&input(), &with(wider)).unwrap();
    let wider_mech = mechanics(&input(), &wider, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert!(wider_mech.axial_thrust.1 > mech.axial_thrust.1);

    let free = design(&input()).unwrap();
    let free_mech = mechanics(&input(), &free, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert_eq!(free_mech.front_seal_diameter, free.d1.si);
}

#[test]
fn the_seal_leakage_uses_the_neck() {
    let tight = design_with_options(&input(), &with(sample())).unwrap();
    let wider = CasingFit {
        neck_diameter: 0.110,
        ..sample()
    };
    let wider = design_with_options(&input(), &with(wider)).unwrap();
    assert!(wider.seal_leakage_ratio > tight.seal_leakage_ratio);
}

#[test]
fn an_eye_with_no_room_around_the_hub_is_an_error() {
    let error = design_with_options(&input(), &with_eye_limit(0.033))
        .unwrap_err()
        .to_string();
    assert!(error.contains("33.0 mm"), "{error}");
    assert!(error.contains("30.0 mm"), "{error}");
}

#[test]
fn the_mechanics_use_the_casing_hub() {
    let thin = design_with_options(&input(), &with(sample())).unwrap();
    let thin_mech = mechanics(&input(), &thin, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert_eq!(thin_mech.hub_diameter, 0.030);
    assert_eq!(thin_mech.shaft_seal_diameter, 0.030);

    let thick = CasingFit {
        hub_diameter: 0.040,
        ..sample()
    };
    let thick = design_with_options(&input(), &with(thick)).unwrap();
    let thick_mech = mechanics(&input(), &thick, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert!(thick_mech.axial_thrust.1 < thin_mech.axial_thrust.1);
    assert_eq!(thick_mech.hub_min_diameter, thin_mech.hub_min_diameter);
}

#[test]
fn without_a_casing_the_hub_is_the_required_one() {
    let free = design(&input()).unwrap();
    let mech = mechanics(&input(), &free, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert_eq!(mech.hub_diameter, mech.hub_min_diameter);
}

#[test]
fn casing_sizes_that_do_not_nest_are_rejected() {
    type Edit = fn(&mut CasingFit);
    let cases: [(Edit, &str); 5] = [
        (|c| c.eye_diameter_max = 0.095, "eye_diameter_max"),
        (|c| c.neck_diameter = 0.220, "neck_diameter"),
        (|c| c.hub_diameter = 0.080, "hub_diameter"),
        (|c| c.bore_diameter = 0.035, "bore_diameter"),
        (|c| c.cutwater_radius = 0.100, "cutwater_radius"),
    ];
    for (edit, field) in cases {
        let mut casing = sample();
        edit(&mut casing);
        let error = design_with_options(&input(), &with(casing))
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!("casing.{field}")), "{error}");
    }
}

fn double() -> CasingFit {
    CasingFit {
        channels: 2,
        ..sample()
    }
}

fn has(result: &hm_design::DesignResult, code: &str) -> bool {
    codes(result).iter().any(|found| found == code)
}

#[test]
fn the_volute_is_the_casing_one() {
    let result = design_with_options(&input(), &with(sample())).unwrap();
    assert_eq!(result.volute.width.si, 0.030);
    assert!((result.volute.base_radius.si - 0.111).abs() < 1e-12);
}

#[test]
fn two_channels_give_an_odd_blade_count_and_less_radial_thrust() {
    let single = design_with_options(&input(), &with(sample())).unwrap();
    let double = design_with_options(&input(), &with(double())).unwrap();
    assert_eq!(double.selection.blade_count % 2, 1);
    assert_eq!(double.volute.wrap_deg, Some(180.0));
    assert_eq!(single.volute.wrap_deg, None);
    assert!(!has(&single, "HM-DES-038"));
    assert!(!has(&double, "HM-DES-038"));

    let shear = DEFAULT_SHAFT_SHEAR_STRESS;
    let thrust = |result| {
        mechanics(&input(), result, shear)
            .unwrap()
            .radial_thrust_design
    };
    assert!(thrust(&double) < thrust(&single));
}

#[test]
fn volute_options_that_agree_pass_and_those_that_disagree_fail() {
    let k3 = 2.0 * 0.111 / 0.210;
    let agreeing = DesignOptions {
        k3: Some(k3),
        volute_channels: Some(2),
        volute_wrap_deg: Some(180.0),
        ..with(double())
    };
    assert!(design_with_options(&input(), &agreeing).is_ok());

    type Edit = fn(&mut DesignOptions);
    let cases: [(Edit, &str); 4] = [
        (|o| o.k3 = Some(1.2), "k3"),
        (|o| o.volute_channels = Some(1), "volute_channels"),
        (|o| o.volute_wrap_deg = Some(170.0), "volute_wrap_deg"),
        (|o| o.volute_width_factor = Some(0.05), "volute_c"),
    ];
    for (edit, name) in cases {
        let mut options = with(double());
        edit(&mut options);
        let error = design_with_options(&input(), &options)
            .unwrap_err()
            .to_string();
        assert!(error.contains(name), "{error}");
        assert!(error.contains("[casing]"), "{error}");
    }
}

#[test]
fn without_a_throat_the_volute_does_not_decide_the_transition() {
    let result = design_with_options(&input(), &with(sample())).unwrap();
    let qd = input().q.si;
    assert!((result.volute_matching_flow.si / qd - 10.0).abs() < 1e-9);
    let inlet = 0.5 * (result.shockless_flow.si + result.throat_flow.si) / qd;
    assert!((result.transition_ratio - inlet).abs() < 1e-9);
    assert!(has(&result, "HM-DES-040"));
}

#[test]
fn a_throat_enters_equation_6_22a_per_channel() {
    let area = 0.0012;
    let fit = |channels, throat_area| CasingFit {
        channels,
        throat_area: Some(throat_area),
        ..sample()
    };
    let one = DesignOptions {
        blade_count: Some(7),
        ..with(fit(1, 2.0 * area))
    };
    let two = DesignOptions {
        blade_count: Some(7),
        ..with(fit(2, area))
    };
    let one = design_with_options(&input(), &one).unwrap();
    let two = design_with_options(&input(), &two).unwrap();
    let qd = input().q.si;
    assert!(one.volute_matching_flow.si / qd < 10.0);
    assert!((one.volute_matching_flow.si / two.volute_matching_flow.si - 1.0).abs() < 1e-9);
    assert!(!has(&two, "HM-DES-040"));
}

#[test]
fn a_smaller_throat_lowers_the_matching_flow() {
    let flow = |area| {
        let fit = CasingFit {
            throat_area: Some(area),
            ..sample()
        };
        design_with_options(&input(), &with(fit))
            .unwrap()
            .volute_matching_flow
            .si
    };
    assert!(flow(0.0015) < flow(0.0020));
}

#[test]
fn an_even_blade_count_in_two_channels_is_still_warned() {
    let options = DesignOptions {
        blade_count: Some(6),
        ..with(double())
    };
    let result = design_with_options(&input(), &options).unwrap();
    assert!(has(&result, "HM-DES-037"));
}
