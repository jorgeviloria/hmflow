//! Diseño de un rodete para una carcasa existente. Carcasa ficticia.

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
