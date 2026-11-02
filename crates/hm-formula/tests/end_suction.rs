use std::collections::BTreeMap;

use hm_formula::{END_SUCTION_V01_PACK, Registry};
use hm_units::{Quantity, parse_quantity};

fn evaluate(id: &str, inputs: &[(&str, &str)]) -> f64 {
    let registry = Registry::from_toml(END_SUCTION_V01_PACK).unwrap();
    let map: BTreeMap<String, Quantity> = inputs
        .iter()
        .map(|(name, value)| (name.to_string(), parse_quantity(value).unwrap()))
        .collect();
    registry.evaluate(id, &map).unwrap().value()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

#[test]
fn pack_is_audit_clean() {
    let issues = Registry::from_toml(END_SUCTION_V01_PACK).unwrap().audit();
    assert!(issues.is_empty(), "problemas de auditoría: {issues:?}");
}

#[test]
fn pack_has_thirty_two_formulas() {
    assert_eq!(Registry::from_toml(END_SUCTION_V01_PACK).unwrap().len(), 32);
}

#[test]
fn all_merges_both_packs() {
    assert_eq!(Registry::all().len(), 42);
}

#[test]
fn hydraulic_efficiency() {
    close(
        evaluate("design.hydraulic_efficiency", &[("Dn", "93.2")]),
        0.870,
        0.002,
    );
}

#[test]
fn volumetric_efficiency() {
    close(
        evaluate("design.volumetric_efficiency", &[("nq", "31.7997")]),
        0.9722,
        0.002,
    );
}

#[test]
fn overall_efficiency() {
    close(
        evaluate(
            "design.overall_efficiency",
            &[("eta_h", "0.870"), ("eta_v", "0.9722"), ("eta_m", "0.96")],
        ),
        0.8120,
        0.003,
    );
}

#[test]
fn u2() {
    close(
        evaluate("design.u2", &[("H", "13.8 m"), ("psi", "0.87")]),
        17.64,
        0.01,
    );
}

#[test]
fn d2() {
    close(
        evaluate(
            "design.d2",
            &[("H", "13.8 m"), ("n", "1800 rpm"), ("psi", "0.87")],
        ),
        0.1872,
        0.001,
    );
}

#[test]
fn qth() {
    close(
        evaluate("design.qth", &[("Q", "0.016 m3/s"), ("eta_v", "0.9722")]),
        0.01646,
        0.0001,
    );
}

#[test]
fn cm2() {
    close(
        evaluate("design.cm2", &[("phi", "0.13"), ("u2", "17.64 m/s")]),
        2.293,
        0.01,
    );
}

#[test]
fn b2() {
    close(
        evaluate(
            "design.b2",
            &[
                ("Qth", "0.01646 m3/s"),
                ("D2", "0.1872 m"),
                ("cm2", "2.293 m/s"),
            ],
        ),
        0.0122,
        0.0002,
    );
}

#[test]
fn d0() {
    close(
        evaluate("design.d0", &[("Qth", "0.01646 m3/s"), ("c0", "3.6 m/s")]),
        0.0763,
        0.001,
    );
}

#[test]
fn d1() {
    close(
        evaluate("design.d1", &[("D0", "0.0763 m"), ("J", "1.03")]),
        0.0786,
        0.001,
    );
}

#[test]
fn b1() {
    close(
        evaluate(
            "design.b1",
            &[
                ("Qth", "0.01646 m3/s"),
                ("D1", "0.0786 m"),
                ("cm0", "2.25 m/s"),
            ],
        ),
        0.0296,
        0.0005,
    );
}

#[test]
fn shaft_power() {
    close(
        evaluate(
            "design.shaft_power",
            &[("P_hyd", "2167.5 W"), ("eta", "0.813")],
        ),
        2666.0,
        5.0,
    );
}

#[test]
fn max_power() {
    close(
        evaluate(
            "design.max_power",
            &[("P_shaft", "2666 W"), ("margin", "1.1")],
        ),
        2932.6,
        5.0,
    );
}

#[test]
fn volute_base_radius() {
    close(
        evaluate(
            "design.volute_base_radius",
            &[("D2", "0.1871 m"), ("k3", "1.04")],
        ),
        0.097292,
        1e-6,
    );
}

#[test]
fn volute_width() {
    close(
        evaluate(
            "design.volute_width",
            &[("B2", "0.0122 m"), ("D2", "0.1871 m"), ("c", "0.05")],
        ),
        0.021555,
        1e-6,
    );
}

#[test]
fn volute_inlet_velocity() {
    close(
        evaluate(
            "design.volute_inlet_velocity",
            &[("cm2", "2.293 m/s"), ("cu2", "10.363 m/s")],
        ),
        10.613,
        0.01,
    );
}

#[test]
fn volute_throat_area() {
    close(
        evaluate(
            "design.volute_throat_area",
            &[("Q", "0.016 m3/s"), ("C3", "10.613 m/s")],
        ),
        0.0015075,
        1e-6,
    );
}

#[test]
fn volute_throat_diameter() {
    close(
        evaluate(
            "design.volute_throat_diameter",
            &[("A360", "0.0015075 m2"), ("k_eff", "1.1")],
        ),
        0.04819,
        1e-4,
    );
}

#[test]
fn volute_outer_radius() {
    close(
        evaluate(
            "design.volute_outer_radius",
            &[("R3", "0.097292 m"), ("D360", "0.04819 m")],
        ),
        0.121387,
        1e-5,
    );
}

#[test]
fn hydraulic_loss() {
    close(
        evaluate(
            "design.hydraulic_loss",
            &[("P_hyd", "2167.5 W"), ("eta_h", "0.87")],
        ),
        323.8,
        0.5,
    );
}

#[test]
fn volumetric_loss() {
    close(
        evaluate(
            "design.volumetric_loss",
            &[
                ("P_hyd", "2167.5 W"),
                ("eta_h", "0.87"),
                ("eta_v", "0.9722"),
            ],
        ),
        71.2,
        0.5,
    );
}

#[test]
fn mechanical_loss() {
    close(
        evaluate(
            "design.mechanical_loss",
            &[
                ("P_hyd", "2167.5 W"),
                ("eta_h", "0.87"),
                ("eta_v", "0.9722"),
                ("eta_m", "0.96"),
            ],
        ),
        106.8,
        0.5,
    );
}

#[test]
fn specific_diameter() {
    close(
        evaluate(
            "design.specific_diameter",
            &[
                ("A", "4.5"),
                ("Q", "0.016 m3/s"),
                ("n", "1800 rpm"),
                ("mm", "0.001 m"),
            ],
        ),
        93.24,
        0.1,
    );
}

#[test]
fn thoma_sigma() {
    close(
        evaluate("design.thoma_sigma", &[("nq", "31.7997")]),
        0.12094,
        1e-4,
    );
}

#[test]
fn npshr_design() {
    close(
        evaluate(
            "design.npshr_design",
            &[("sigma", "0.12094"), ("H", "13.8 m")],
        ),
        1.669,
        0.01,
    );
}

#[test]
fn shutoff_head() {
    close(
        // ψo = 1,25·e^(−0,0954) = 1,1363; H0 = ψo·17,64²/(2·9,80665)
        evaluate(
            "design.shutoff_head",
            &[("nq", "31.7997"), ("u2", "17.64 m/s")],
        ),
        18.03,
        0.01,
    );
}

#[test]
fn slip_factor() {
    // 0,98·(1 − √(sen 23,5°)/7^0,7)
    close(
        evaluate("design.slip_factor", &[("beta2", "23.5 deg"), ("Z", "7")]),
        0.82151,
        1e-4,
    );
}

#[test]
fn cu2() {
    // 30·0,8 − 4/tan 30° = 24 − 6,9282
    close(
        evaluate(
            "design.cu2",
            &[
                ("slip", "0.8"),
                ("u2", "30 m/s"),
                ("cm2", "4 m/s"),
                ("beta2", "30 deg"),
            ],
        ),
        17.0718,
        1e-3,
    );
}

#[test]
fn head_coefficient() {
    // 1,21·e^(−0,77·0,318)
    close(
        evaluate("design.head_coefficient", &[("nq", "31.8")]),
        0.94721,
        1e-4,
    );
}

#[test]
fn relative_outlet_width() {
    // 0,017 + 0,262·0,318 − 0,08·0,318² + 0,0093·0,318³
    close(
        evaluate("design.relative_outlet_width", &[("nq", "31.8")]),
        0.09253,
        1e-4,
    );
}

#[test]
fn flow_coefficient() {
    // D1.A1: nq = 31,8, ψ = 0,87, ηv = 0,972 y b2/d2 = 0,0122/0,1871.
    close(
        evaluate(
            "design.flow_coefficient",
            &[
                ("nq", "31.8"),
                ("psi", "0.87"),
                ("eta_v", "0.972"),
                ("b2_star", "0.065206"),
            ],
        ),
        0.13,
        0.001,
    );
}

#[test]
fn euler_cu2() {
    // D1.A1: 9,80665·13,8/(0,871·17,64)
    close(
        evaluate(
            "design.euler_cu2",
            &[("H", "13.8 m"), ("eta_h", "0.871"), ("u2", "17.64 m/s")],
        ),
        8.8083,
        1e-3,
    );
}
