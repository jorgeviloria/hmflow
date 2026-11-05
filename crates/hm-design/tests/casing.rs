//! Diseño de un rodete para una carcasa existente. Carcasa ficticia.

use hm_design::{CasingFit, DesignInput, DesignOptions, design, design_with_options};
use hm_units::parse_quantity;

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
        d2: 0.200,
        neck_diameter: 0.090,
        eye_diameter_max: 0.075,
        bore_diameter: 0.020,
        hub_diameter: 0.030,
        b3: 0.030,
        cutwater_radius: 0.106,
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
    assert!((result.d2.si - 0.200).abs() < 1e-12, "{}", result.d2.si);
    assert!((result.selection.psi - 0.956_996).abs() < 1e-5);
    let u2 = std::f64::consts::PI * 0.200 * 2900.0 / 60.0;
    assert!((result.u2.si - u2).abs() < 1e-9);
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
    assert!(error.contains("0.957"), "{error}");
    assert!(error.contains("[casing]"), "{error}");
}

#[test]
fn an_impossible_d2_gives_the_euler_error() {
    let casing = CasingFit {
        d2: 0.150,
        ..sample()
    };
    let error = design_with_options(&input(), &with(casing))
        .unwrap_err()
        .to_string();
    assert!(error.contains("ningún β2 da la altura"), "{error}");
}

#[test]
fn without_casing_nothing_changes() {
    let plain = design(&input()).unwrap();
    let none = design_with_options(&input(), &DesignOptions::default()).unwrap();
    assert_eq!(plain.d2.si, none.d2.si);
    assert!((plain.d2.si - 0.2).abs() > 1e-6);
}
