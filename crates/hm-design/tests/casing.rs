//! Diseño de un rodete para una carcasa existente. Carcasa ficticia.

use hm_design::geometry::geometry;
use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics, mechanics_diagnostics};
use hm_design::{
    CasingFit, CasingLeakage, DesignInput, DesignOptions, design, design_with_options,
};
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
        wrap_deg: None,
        throat_area: None,
        leakage: None,
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
        (
            |c| {
                c.channels = 2;
                c.wrap_deg = Some(200.0);
            },
            "wrap",
        ),
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
    let cases: [(Edit, &str); 6] = [
        (|c| c.cutwater_radius = 0.105, "cutwater_radius"),
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
        wrap_deg: Some(180.0),
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
    let cases: [(Edit, &str); 5] = [
        (|o| o.volute_ratio = Some(0.8), "volute_ratio"),
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
        wrap_deg: (channels == 2).then_some(180.0),
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

#[test]
fn the_wrap_goes_with_two_channels_only() {
    let missing = CasingFit {
        wrap_deg: None,
        ..double()
    };
    let error = design_with_options(&input(), &with(missing))
        .unwrap_err()
        .to_string();
    assert!(error.contains("casing.wrap"), "{error}");

    let spare = CasingFit {
        wrap_deg: Some(180.0),
        ..sample()
    };
    let error = design_with_options(&input(), &with(spare))
        .unwrap_err()
        .to_string();
    assert!(error.contains("casing.wrap"), "{error}");
}

#[test]
fn an_impeller_wider_than_the_casing_inlet_is_an_error() {
    let narrow = CasingFit {
        b3: 0.010,
        ..sample()
    };
    let error = design_with_options(&input(), &with(narrow))
        .unwrap_err()
        .to_string();
    assert!(error.contains("b2"), "{error}");
    assert!(error.contains("10.0 mm"), "{error}");
}

/// Ec. (6.22a) calculada aparte con u2, A2, deslizamiento, τ2 y β2 del diseño.
#[test]
fn the_matching_flow_matches_the_hand_calculation() {
    let casing = CasingFit {
        throat_area: Some(0.0012),
        ..sample()
    };
    let options = DesignOptions {
        blade_count: Some(7),
        ..with(casing)
    };
    let result = design_with_options(&input(), &options).unwrap();
    assert!((result.volute_matching_flow.si - 0.022_970_29).abs() < 1e-8);
}

fn fit_codes(casing: CasingFit) -> Vec<String> {
    let result = design_with_options(&input(), &with(casing)).unwrap();
    let mech = mechanics(&input(), &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    mechanics_diagnostics(&mech, &result)
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect()
}

fn fires(casing: CasingFit, code: &str) -> bool {
    fit_codes(casing).iter().any(|found| found == code)
}

/// Una carcasa de holguras amplias: no dispara ninguna comprobación.
fn roomy() -> CasingFit {
    CasingFit {
        neck_diameter: 0.120,
        bore_diameter: 0.029,
        hub_diameter: 0.035,
        ..sample()
    }
}

#[test]
fn a_roomy_casing_raises_no_fit_warning() {
    let found = fit_codes(roomy());
    for code in ["HM-DES-041", "HM-DES-042", "HM-DES-043", "HM-DES-044"] {
        assert!(!found.iter().any(|item| item == code), "{code}: {found:?}");
    }
}

#[test]
fn a_neck_with_no_wall_is_warned() {
    let tight = CasingFit {
        neck_diameter: 0.076,
        ..roomy()
    };
    assert!(fires(tight, "HM-DES-041"));
}

#[test]
fn a_bore_under_the_minimum_shaft_is_warned_with_the_criterion() {
    let casing = CasingFit {
        bore_diameter: 0.020,
        ..roomy()
    };
    let result = design_with_options(&input(), &with(casing)).unwrap();
    let mech = mechanics(&input(), &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let diagnostics = mechanics_diagnostics(&mech, &result);
    let warning = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "HM-DES-042")
        .unwrap();
    assert!(warning.message.contains("20.000 mm"), "{}", warning.message);
    assert!(warning.message.contains("criterio"), "{}", warning.message);
}

#[test]
fn a_hub_under_the_minimum_is_warned_with_the_criterion() {
    let casing = CasingFit {
        hub_diameter: 0.030,
        ..roomy()
    };
    let result = design_with_options(&input(), &with(casing)).unwrap();
    let mech = mechanics(&input(), &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let diagnostics = mechanics_diagnostics(&mech, &result);
    let warning = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "HM-DES-044")
        .unwrap();
    assert!(warning.message.contains("criterio"), "{}", warning.message);
}

#[test]
fn every_fit_warning_at_once_still_gives_a_design() {
    let casing = CasingFit {
        neck_diameter: 0.076,
        bore_diameter: 0.020,
        hub_diameter: 0.030,
        b3: 0.019,
        ..sample()
    };
    let found = fit_codes(casing);
    for code in ["HM-DES-041", "HM-DES-042", "HM-DES-043", "HM-DES-044"] {
        assert!(found.iter().any(|item| item == code), "{code}: {found:?}");
    }
}

#[test]
fn a_volute_width_outside_the_book_values_is_only_informed() {
    for b3 in [0.019, 0.080] {
        let casing = CasingFit { b3, ..roomy() };
        let result = design_with_options(&input(), &with(casing.clone())).unwrap();
        assert!(result.b2.si < b3);
        let mech = mechanics(&input(), &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
        let diagnostics = mechanics_diagnostics(&mech, &result);
        let note = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "HM-DES-043")
            .unwrap();
        assert_eq!(note.severity, hm_core::Severity::Info);
    }
}

#[test]
fn the_fit_checks_need_a_casing() {
    let free = design(&input()).unwrap();
    let mech = mechanics(&input(), &free, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let codes: Vec<_> = mechanics_diagnostics(&mech, &free)
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect();
    for code in ["HM-DES-041", "HM-DES-042", "HM-DES-043", "HM-DES-044"] {
        assert!(!codes.iter().any(|item| item == code), "{code}");
    }
}

fn hint_of(result: &hm_design::DesignResult, code: &str) -> String {
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == code)
        .unwrap_or_else(|| panic!("falta {code}: {:?}", codes(result)));
    format!(
        "{} {}",
        diagnostic.message,
        diagnostic.hint.clone().unwrap_or_default()
    )
}

#[test]
fn no_warning_asks_for_what_the_casing_fixes() {
    // Lengua pegada al rodete: HM-DES-023.
    let result = design_with_options(&input(), &with(sample())).unwrap();
    let cutwater = hint_of(&result, "HM-DES-023");
    assert!(!cutwater.contains("k3"), "{cutwater}");

    // D2 corto: ψ alto y curva plana.
    let small = CasingFit {
        d2: 0.190,
        neck_diameter: 0.150,
        eye_diameter_max: 0.100,
        cutwater_radius: 0.100,
        ..sample()
    };
    let result = design_with_options(&input(), &with(small)).unwrap();
    for diagnostic in result.diagnostics.iter() {
        let text = format!("{} {:?}", diagnostic.message, diagnostic.hint);
        assert!(!text.contains("reduce ψ"), "{text}");
        assert!(!text.contains("reduce el"), "{text}");
        assert!(!text.contains("estrecha la voluta"), "{text}");
        assert!(!text.contains("Aumenta k3"), "{text}");
    }
}

#[test]
fn without_a_casing_the_hints_stay() {
    let options = DesignOptions {
        k3: Some(1.02),
        ..DesignOptions::default()
    };
    let result = design_with_options(&input(), &options).unwrap();
    assert!(hint_of(&result, "HM-DES-023").contains("Aumenta k3"));
}

#[test]
fn the_model_outlet_cannot_block_a_casing_design() {
    let q = |text: &str| parse_quantity(text).unwrap();
    let big = DesignInput {
        q: q("30 m3/s"),
        h: q("30 m"),
        n: q("200 rpm"),
        ..input()
    };
    let free = design(&big).unwrap();
    assert!(geometry(&big, &free).is_err());

    let d2 = free.d2.si;
    let casing = CasingFit {
        d2,
        neck_diameter: 0.70 * d2,
        eye_diameter_max: 0.65 * d2,
        bore_diameter: 0.10 * d2,
        hub_diameter: 0.20 * d2,
        b3: 2.0 * free.b2.si,
        cutwater_radius: 0.54 * d2,
        channels: 1,
        wrap_deg: None,
        throat_area: None,
        leakage: None,
    };
    let fitted = design_with_options(&big, &with(casing)).unwrap();
    let shape = geometry(&big, &fitted).unwrap();
    assert_eq!(shape.outlet.cutwater_radius, 0.54 * d2);
}

#[test]
fn a_casing_too_small_for_the_head_blames_the_casing() {
    // D2 corto: ψ pasa de ψo y la altura a caudal cero cae bajo la de diseño.
    let small = CasingFit {
        d2: 0.175,
        neck_diameter: 0.150,
        eye_diameter_max: 0.100,
        cutwater_radius: 0.092,
        ..sample()
    };
    let result = design_with_options(&input(), &with(small)).unwrap();
    let hint = hint_of(&result, "HM-DES-016");
    assert!(hint.contains("esta carcasa"), "{hint}");
    assert!(!hint.contains("reduce el"), "{hint}");
}

fn leakage() -> CasingLeakage {
    CasingLeakage {
        seal_clearance: 0.0003,
        seal_length: 0.015,
        rear_seal_diameter: 0.080,
        rear_seal_length: 0.015,
        balance_hole_count: 6,
        balance_hole_diameter: 0.009,
        balance_hole_circle: 0.050,
    }
}

#[test]
fn bad_leakage_data_is_rejected_by_name() {
    type Edit = fn(&mut CasingLeakage);
    let cases: [(Edit, &str); 15] = [
        (|l| l.balance_hole_circle = 0.0, "balance_hole_circle"),
        (|l| l.seal_clearance = 0.020, "seal_clearance"),
        (|l| l.seal_length = 0.0001, "seal_clearance"),
        (|l| l.seal_length = 0.100, "seal_length"),
        (|l| l.rear_seal_length = 0.100, "rear_seal_length"),
        (|l| l.balance_hole_diameter = 0.060, "balance_hole_diameter"),
        (|l| l.balance_hole_count = 500, "balance_hole_count"),
        (|l| l.balance_hole_circle = 0.035, "balance_hole_circle"),
        (|l| l.seal_clearance = 0.0, "seal_clearance"),
        (|l| l.seal_length = -1.0, "seal_length"),
        (|l| l.rear_seal_diameter = f64::NAN, "rear_seal_diameter"),
        (|l| l.rear_seal_diameter = 0.030, "rear_seal_diameter"),
        (|l| l.rear_seal_diameter = 0.210, "rear_seal_diameter"),
        (|l| l.balance_hole_count = 0, "balance_hole_count"),
        (|l| l.balance_hole_diameter = 0.0, "balance_hole_diameter"),
    ];
    for (edit, field) in cases {
        let mut data = leakage();
        edit(&mut data);
        let casing = CasingFit {
            leakage: Some(data),
            ..sample()
        };
        let error = design_with_options(&input(), &with(casing))
            .unwrap_err()
            .to_string();
        assert!(error.contains(&format!("casing.{field}")), "{error}");
    }
}

fn leaky() -> CasingFit {
    CasingFit {
        leakage: Some(leakage()),
        ..sample()
    }
}

#[test]
fn the_declared_clearance_is_the_one_shown_and_contrasted() {
    let plain = design_with_options(&input(), &with(sample())).unwrap();
    let fitted = design_with_options(&input(), &with(leaky())).unwrap();
    let mech = mechanics(&input(), &fitted, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    assert_eq!(mech.seal_clearance, 0.0003);
    assert!(fitted.seal_leakage_ratio < plain.seal_leakage_ratio);
}

#[test]
fn two_clearances_that_disagree_are_an_error() {
    let options = DesignOptions {
        seal_clearance: Some(0.0005),
        ..with(leaky())
    };
    let error = design_with_options(&input(), &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("seal_clearance"), "{error}");
    assert!(error.contains("[casing]"), "{error}");

    let agreeing = DesignOptions {
        seal_clearance: Some(0.0003),
        ..with(leaky())
    };
    assert!(design_with_options(&input(), &agreeing).is_ok());
}

#[test]
fn a_clearance_far_from_the_usual_is_informed() {
    let tight = CasingFit {
        leakage: Some(CasingLeakage {
            seal_clearance: 0.0002,
            ..leakage()
        }),
        ..sample()
    };
    let found = fit_codes(tight);
    assert!(found.iter().any(|code| code == "HM-DES-045"), "{found:?}");

    let result = design_with_options(&input(), &with(sample())).unwrap();
    let mech = mechanics(&input(), &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let usual = mech.seal_clearance;
    let close = CasingFit {
        leakage: Some(CasingLeakage {
            seal_clearance: usual,
            ..leakage()
        }),
        ..sample()
    };
    assert!(!fit_codes(close).iter().any(|code| code == "HM-DES-045"));
}

#[test]
fn eta_v_comes_from_the_real_leakage_and_converges() {
    let result = design_with_options(&input(), &with(leaky())).unwrap();
    let total = 1.0 / (1.0 + result.seal_leakage_ratio + result.rear_seal_leakage_ratio);
    assert!(
        (result.eta_v - total).abs() < 1e-8,
        "{} {total}",
        result.eta_v
    );
    assert_eq!(result.leakage_eta_v, result.eta_v);
    assert!(result.rear_seal_leakage_ratio > 0.0);
}

/// Calculado aparte con la tabla 3.7 de Gülich, con u2, cu2 y ν del diseño.
#[test]
fn the_leakage_matches_the_hand_calculation() {
    let result = design_with_options(&input(), &with(leaky())).unwrap();
    assert!((result.seal_leakage_ratio - 0.012_280).abs() < 2e-6);
    assert!((result.rear_seal_leakage_ratio - 0.010_930).abs() < 2e-6);
    assert!((result.eta_v - 0.977_317).abs() < 2e-6);
}

#[test]
fn without_leakage_data_eta_v_stays_the_statistical_one() {
    let plain = design_with_options(&input(), &with(sample())).unwrap();
    assert_eq!(plain.rear_seal_leakage_ratio, 0.0);
    let free = design(&input()).unwrap();
    assert_eq!(plain.eta_v, free.eta_v);
}

#[test]
fn a_written_eta_v_wins_over_the_leakage() {
    let options = DesignOptions {
        eta_v: Some(0.95),
        ..with(leaky())
    };
    let result = design_with_options(&input(), &options).unwrap();
    assert_eq!(result.eta_v, 0.95);
}

#[test]
fn the_balance_holes_flag_does_not_count_the_leakage_twice() {
    let options = DesignOptions {
        balance_holes: true,
        ..with(leaky())
    };
    let flagged = design_with_options(&input(), &options).unwrap();
    let plain = design_with_options(&input(), &with(leaky())).unwrap();
    assert!((flagged.eta_v - plain.eta_v).abs() < 1e-9);
}

#[test]
fn longer_rings_leak_less() {
    let longer = CasingFit {
        leakage: Some(CasingLeakage {
            seal_length: 0.030,
            rear_seal_length: 0.030,
            ..leakage()
        }),
        ..sample()
    };
    let long = design_with_options(&input(), &with(longer)).unwrap();
    let short = design_with_options(&input(), &with(leaky())).unwrap();
    assert!(long.seal_leakage_ratio < short.seal_leakage_ratio);
    assert!(long.rear_seal_leakage_ratio < short.rear_seal_leakage_ratio);
    assert!(long.eta_v > short.eta_v);
}

#[test]
fn a_seal_right_at_the_laminar_transition_still_gives_a_leakage() {
    // Con esta holgura y este largo, λ salta entre laminar y turbulento justo
    // en la solución.
    let at = |length: f64| {
        let casing = CasingFit {
            leakage: Some(CasingLeakage {
                seal_clearance: 0.0002,
                seal_length: length,
                rear_seal_length: length,
                ..leakage()
            }),
            ..sample()
        };
        design_with_options(&input(), &with(casing))
            .unwrap()
            .seal_leakage_ratio
    };
    let (shorter, jump, longer) = (at(0.012), at(0.015), at(0.020));
    assert!(shorter > jump && jump > longer, "{shorter} {jump} {longer}");
}

fn oversized(rear: f64, neck: f64) -> CasingFit {
    CasingFit {
        d2: 0.40,
        neck_diameter: neck,
        cutwater_radius: 0.212,
        b3: 0.10,
        leakage: Some(CasingLeakage {
            rear_seal_diameter: rear,
            ..leakage()
        }),
        ..sample()
    }
}

#[test]
fn with_no_drive_on_either_ring_eta_v_stays_statistical_and_says_so() {
    let result = design_with_options(&input(), &with(oversized(0.080, 0.090))).unwrap();
    let plain = CasingFit {
        leakage: None,
        ..oversized(0.080, 0.090)
    };
    let statistical = design_with_options(&input(), &with(plain)).unwrap();
    assert_eq!(result.eta_v, statistical.eta_v);
    let notes = hint_of(&result, "HM-DES-032");
    assert!(notes.contains("delantero"), "{notes}");
    assert!(notes.contains("trasero"), "{notes}");
}

#[test]
fn a_ring_without_drive_is_named_even_if_the_other_has_it() {
    let result = design_with_options(&input(), &with(oversized(0.080, 0.200))).unwrap();
    assert_eq!(result.rear_seal_leakage_ratio, 0.0);
    assert!(result.seal_leakage_ratio > 0.0);
    let notes = hint_of(&result, "HM-DES-032");
    assert!(notes.contains("trasero"), "{notes}");
    assert!(!notes.contains("delantero"), "{notes}");
}

/// El ηv mueve φ por defecto, y φ puede mover Z, que es entero: la iteración no
/// debe quedarse saltando entre dos valores.
#[test]
fn the_eta_v_iteration_never_fails_to_converge() {
    let q = |text: String| parse_quantity(&text).unwrap();
    let mut designs = 0;
    for flow in [40.0, 80.0, 120.0, 220.0, 400.0] {
        for head in [25.0, 45.0, 80.0] {
            for speed in [1750.0, 2900.0] {
                let point = DesignInput {
                    q: q(format!("{flow} m3/h")),
                    h: q(format!("{head} m")),
                    n: q(format!("{speed} rpm")),
                    ..input()
                };
                let Ok(free) = design(&point) else { continue };
                let d2 = free.d2.si;
                let neck = free.d1.si * 1.3;
                let casing = CasingFit {
                    d2,
                    neck_diameter: neck,
                    eye_diameter_max: free.d1.si * 1.15,
                    bore_diameter: 0.10 * d2,
                    hub_diameter: 0.15 * d2,
                    b3: free.b2.si * 1.9,
                    cutwater_radius: 0.53 * d2,
                    channels: 1,
                    wrap_deg: None,
                    throat_area: None,
                    leakage: Some(CasingLeakage {
                        seal_clearance: 0.003 * neck,
                        seal_length: 0.1 * neck,
                        rear_seal_diameter: 0.5 * (neck + d2),
                        rear_seal_length: 0.1 * neck,
                        balance_hole_count: 6,
                        balance_hole_diameter: 0.05 * neck,
                        balance_hole_circle: 0.3 * d2,
                    }),
                };
                match design_with_options(&point, &with(casing)) {
                    Ok(_) => designs += 1,
                    Err(error) => assert!(
                        !error.to_string().contains("no converge"),
                        "{flow} {head} {speed}: {error}"
                    ),
                }
            }
        }
    }
    assert!(designs >= 15, "solo {designs} diseños");
}

fn with_holes(count: u32, diameter: f64) -> CasingFit {
    CasingFit {
        leakage: Some(CasingLeakage {
            balance_hole_count: count,
            balance_hole_diameter: diameter,
            ..leakage()
        }),
        ..roomy()
    }
}

fn fit_note(casing: CasingFit, code: &str) -> Option<String> {
    let result = design_with_options(&input(), &with(casing)).unwrap();
    let mech = mechanics(&input(), &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    mechanics_diagnostics(&mech, &result)
        .iter()
        .find(|diagnostic| diagnostic.code == code)
        .map(|diagnostic| diagnostic.message.clone())
}

/// Gülich p. 657: suma de áreas ≥ 4 veces π·ds2·s, con s = 2·s0 por el desgaste.
#[test]
fn small_balance_holes_are_warned_counting_the_worn_seal() {
    // 6×Ø7 = 231 mm²; paso gastado = π·80·0,30 = 75,4 mm²; razón 3,06.
    let note = fit_note(with_holes(6, 0.007), "HM-DES-046").unwrap();
    assert!(note.contains("3.06"), "{note}");
    // Con el anillo nuevo pasaría: 231/37,7 = 6,1. El desgaste es lo que dispara.
    assert_eq!(fit_note(with_holes(6, 0.009), "HM-DES-046"), None);
}

#[test]
fn a_hole_per_channel_is_the_usual() {
    let z = design_with_options(&input(), &with(with_holes(5, 0.009)))
        .unwrap()
        .selection
        .blade_count;
    assert_eq!(fit_note(with_holes(z, 0.009), "HM-DES-047"), None);
    let note = fit_note(with_holes(z + 1, 0.009), "HM-DES-047").unwrap();
    assert!(note.contains(&format!("Z = {z}")), "{note}");
}

#[test]
fn hole_checks_need_leakage_data() {
    assert_eq!(fit_note(roomy(), "HM-DES-046"), None);
    assert_eq!(fit_note(roomy(), "HM-DES-047"), None);
}
