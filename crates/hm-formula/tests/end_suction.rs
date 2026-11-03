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
fn pack_has_seventy_six_formulas() {
    assert_eq!(Registry::from_toml(END_SUCTION_V01_PACK).unwrap().len(), 76);
}

#[test]
fn all_merges_both_packs() {
    assert_eq!(Registry::all().len(), 86);
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
    // 0,98·(1 − √(sen 23,5°)/7^0,7). eps = 0 deja kw = 1.
    close(
        evaluate(
            "design.slip_factor",
            &[("beta2", "23.5 deg"), ("Z", "7"), ("eps", "0")],
        ),
        0.82151,
        1e-4,
    );
    // εLim = exp(−8,16·sen 23,5°/7) ≈ 0,628. Con eps = 0,9, kw < 1.
    let base = evaluate(
        "design.slip_factor",
        &[("beta2", "23.5 deg"), ("Z", "7"), ("eps", "0")],
    );
    let limited = evaluate(
        "design.slip_factor",
        &[("beta2", "23.5 deg"), ("Z", "7"), ("eps", "0.9")],
    );
    let sin_beta = 23.5_f64.to_radians().sin();
    let eps_lim = (-8.16 * sin_beta / 7.0).exp();
    let kw = 1.0 - ((0.9 - eps_lim) / (1.0 - eps_lim)).powi(3);
    close(limited, base * kw, 1e-4);
    assert!(limited < base);
}

#[test]
fn cu2() {
    // 30·0,8 − 4/tan 30° = 24 − 6,9282 con τ2 = 1.
    close(
        evaluate(
            "design.cu2",
            &[
                ("slip", "0.8"),
                ("u2", "30 m/s"),
                ("cm2", "4 m/s"),
                ("beta2", "30 deg"),
                ("tau", "1"),
            ],
        ),
        17.0718,
        1e-3,
    );
    // Con τ2 = 1,2 el bloqueo resta 1,2 veces la misma meridional local.
    close(
        evaluate(
            "design.cu2",
            &[
                ("slip", "0.8"),
                ("u2", "30 m/s"),
                ("cm2", "4 m/s"),
                ("beta2", "30 deg"),
                ("tau", "1.2"),
            ],
        ),
        24.0 - 1.2 * 4.0 / 30.0_f64.to_radians().tan(),
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

#[test]
fn viscous_reynolds() {
    // 2π·48,333·0,1²/1e−4 · (30/52,9)^1,5
    close(
        evaluate(
            "design.viscous_reynolds",
            &[
                ("n", "2900 rpm"),
                ("d2", "0.2 m"),
                ("nu", "100 cSt"),
                ("nq", "30"),
            ],
        ),
        12969.5,
        0.5,
    );
}

#[test]
fn viscous_head_factor() {
    // 1e4^(−6,7/1e4^0,735)
    close(
        evaluate("design.viscous_head_factor", &[("Re_mod", "1e4")]),
        0.93160,
        1e-4,
    );
}

#[test]
fn viscous_efficiency_factor() {
    // 1e4^(−19/1e4^0,705)
    close(
        evaluate("design.viscous_efficiency_factor", &[("Re_mod", "1e4")]),
        0.76731,
        1e-4,
    );
}

#[test]
fn cutwater_ratio() {
    // 1,03 + 0,1·30/40 + 0,07·998,2·45/10⁶
    close(
        evaluate(
            "design.cutwater_ratio",
            &[
                ("nq", "30"),
                ("rho", "998.2 kg/m3"),
                ("H", "45 m"),
                ("rho_ref", "1000 kg/m3"),
                ("H_ref", "1000 m"),
            ],
        ),
        1.108144,
        1e-6,
    );
}

#[test]
fn diffuser_allowable_angle() {
    // L/R1 = 1 da 16,5° (2ϑ = 33°, curva a de la fig. 1.20); L/R1 = 4, 8,25°.
    close(
        evaluate(
            "design.diffuser_allowable_angle",
            &[("R1", "0.025 m"), ("L", "0.025 m")],
        ),
        16.5,
        1e-9,
    );
    close(
        evaluate(
            "design.diffuser_allowable_angle",
            &[("R1", "0.025 m"), ("L", "0.1 m")],
        ),
        8.25,
        1e-9,
    );
}

#[test]
fn blade_inlet_angle() {
    // D1.A1: arctan(2,25/7,41) = 16,89° (el libro escribe 16,9°) más i1 = 2°.
    close(
        evaluate(
            "design.blade_inlet_angle",
            &[("c1m", "2.25 m/s"), ("u1", "7.41 m/s"), ("i1", "2 deg")],
        ),
        18.891,
        1e-3,
    );
}

#[test]
fn shockless_flow() {
    // 0,9714·π·0,1135·0,0428·tan 9,44°·17,23
    close(
        evaluate(
            "design.shockless_flow",
            &[
                ("d1", "0.1135 m"),
                ("b1", "0.0428 m"),
                ("beta1b", "9.44 deg"),
                ("u1", "17.23 m/s"),
                ("eta_v", "0.9714"),
                ("tau", "1"),
            ],
        ),
        0.042469,
        1e-5,
    );
}

#[test]
fn throat_flow() {
    // Sin espesor, zLa·A1q = A1·sen β1B y QW = QSF.
    let a1 = std::f64::consts::PI * 0.1135 * 0.0428;
    let a1q = a1 * 9.44f64.to_radians().sin();
    close(
        evaluate(
            "design.throat_flow",
            &[
                ("A1", &format!("{a1} m2")),
                ("A1q", &format!("{a1q} m2")),
                ("u1m", "17.23 m/s"),
                ("eta_v", "0.9714"),
            ],
        ),
        0.042469,
        1e-5,
    );
}

#[test]
fn volute_matching_flow() {
    // 30,37·A2·0,8365/(A2/0,001975 + 1/(0,9714·tan 19,08°)), A2 = π·0,2·0,01794
    let a2 = std::f64::consts::PI * 0.2 * 0.01794;
    close(
        evaluate(
            "design.volute_matching_flow",
            &[
                ("u2", "30.37 m/s"),
                ("A2", &format!("{a2} m2")),
                ("slip", "0.8365"),
                ("A3q", "0.001975 m2"),
                ("eta_v", "0.9714"),
                ("beta2", "19.08 deg"),
                ("tau", "1"),
            ],
        ),
        0.032977,
        1e-5,
    );
}

#[test]
fn volute_matching_flow_cm() {
    // Gülich 6.22a: u2·A2·γ·(√(a² − 1) − τ/tan β2)/(a² − 1 − (τ/tan β2)²)
    // con u2 = 30 m/s, A2 = 0,02 m², γ = 0,8, a = 2, τ = 1,05 y β2 = 20°.
    close(
        evaluate(
            "design.volute_matching_flow_cm",
            &[
                ("u2", "30 m/s"),
                ("A2", "0.02 m2"),
                ("slip", "0.8"),
                ("a", "2"),
                ("tau", "1.05"),
                ("beta2", "20 deg"),
            ],
        ),
        0.103_966,
        1e-6,
    );
}

#[test]
fn volute_throat_area_ratio() {
    // T7.7.11 con c3q = ratio·c2: A = Q/(ratio·c2) = 0,1/(0,8·20).
    close(
        evaluate(
            "design.volute_throat_area_ratio",
            &[("Q", "0.1 m3/s"), ("c2", "20 m/s"), ("ratio", "0.8")],
        ),
        0.006_25,
        1e-9,
    );
}

#[test]
fn volute_momentum_throat_diameter() {
    // T7.7.7 con εsp = 2π: X = Q/(π·c2u·r2), d3q = X + √(2·dz'·X).
    // Q = 0,06688 m3/s, c2u = 23,9 m/s, r2 = 0,21325 m, dz' = 0,46705 m.
    close(
        evaluate(
            "design.volute_momentum_throat_diameter",
            &[
                ("Q", "0.06688 m3/s"),
                ("c2u", "23.9 m/s"),
                ("r2", "0.21325 m"),
                ("dz", "0.46705 m"),
                ("eps_sp", "6.283185307179586"),
            ],
        ),
        0.066_641,
        1e-6,
    );
}

#[test]
fn recirculation_onset() {
    close(
        evaluate("design.recirculation_onset", &[("nq", "10")]),
        0.4,
        1e-12,
    );
    close(
        evaluate("design.recirculation_onset", &[("nq", "300")]),
        0.75,
        1e-12,
    );
    close(
        evaluate("design.recirculation_onset", &[("nq", "1000")]),
        0.75,
        1e-12,
    );
    close(
        evaluate("design.recirculation_onset", &[("nq", "30")]),
        0.51305,
        1e-4,
    );
}

#[test]
fn volumetric_efficiency_gulich() {
    // ηv = 1/(1 + a·zH/nq^m), Gülich 3.5.9 y 3.5.10; (a, m) = (4,1, 1,6) con
    // nq < 27 y (0,15, 0,6) con nq ≥ 27; zH = 2 con agujeros de equilibrio.
    let eta = |nq: &str, zh: &str, a: &str, m: &str| {
        evaluate(
            "design.volumetric_efficiency_gulich",
            &[("nq", nq), ("zH", zh), ("a", a), ("m", m)],
        )
    };
    close(eta("10", "1", "4.1", "1.6"), 0.90663, 1e-5);
    close(eta("16.46", "1", "4.1", "1.6"), 0.95566, 1e-5);
    close(eta("100", "1", "0.15", "0.6"), 0.99062, 1e-5);
    close(eta("10", "2", "4.1", "1.6"), 0.82920, 1e-5);
}

#[test]
fn leakage_volumetric_efficiency() {
    // 1/(1 + 4,1·30^−1,6)
    close(
        evaluate("design.leakage_volumetric_efficiency", &[("nq", "30")]),
        0.98255,
        1e-4,
    );
}

#[test]
fn mechanical_efficiency_estimate() {
    // 1 − 0,0045·(1/0,03333)^0,4·(1500/2900)^0,3
    close(
        evaluate(
            "design.mechanical_efficiency_estimate",
            &[
                ("Q", "120 m3/h"),
                ("n", "2900 rpm"),
                ("Q_ref", "1 m3/s"),
                ("n_ref", "1500 rpm"),
            ],
        ),
        0.98561,
        1e-4,
    );
}

#[test]
fn shaft_min_diameter() {
    // D1.A1: T = 15,56 N·m, τ = 180/8 MPa → 15,212 mm.
    close(
        evaluate(
            "mech.shaft_min_diameter",
            &[("T", "15.56 N*m"), ("tau", "22500000 Pa")],
        ),
        0.015212,
        1e-5,
    );
    // D1.B: T = 369,2395 N·m, τ = 40,4/4 MPa → 57,10 mm. El libro escribe
    // 58,7 mm, pero sus propios datos dan 57,10 (errata; toma 60 mm).
    close(
        evaluate(
            "mech.shaft_min_diameter",
            &[("T", "369.2395 N*m"), ("tau", "10100000 Pa")],
        ),
        0.057102,
        1e-6,
    );
}

#[test]
fn hub_diameter() {
    close(
        evaluate("mech.hub_diameter", &[("ds", "0.025 m"), ("k_hub", "1.25")]),
        0.03125,
        1e-12,
    );
    close(
        evaluate("mech.hub_diameter", &[("ds", "0.06 m"), ("k_hub", "1.3")]),
        0.078,
        1e-12,
    );
}

#[test]
fn radial_shutoff_coefficient() {
    // Curva 1 de la fig. 9.46: ≈ 0,089 en nq 10, ≈ 0,375 en el máximo
    // (nq ≈ 57) y ≈ 0,29 en nq 95.
    close(
        evaluate("mech.radial_shutoff_coefficient", &[("nq", "10")]),
        0.089049,
        1e-6,
    );
    close(
        evaluate("mech.radial_shutoff_coefficient", &[("nq", "60")]),
        0.373724,
        1e-6,
    );
    close(
        evaluate("mech.radial_shutoff_coefficient", &[("nq", "95")]),
        0.292923,
        1e-6,
    );
}

#[test]
fn radial_thrust_coefficient() {
    let k = |q: &str| {
        evaluate(
            "mech.radial_thrust_coefficient",
            &[("q", q), ("kR0", "0.3"), ("kR_opt", "0.08")],
        )
    };
    close(k("0"), 0.3, 1e-12);
    // (0,3 − 0,08)(1 − 0,25) + 0,08
    close(k("0.5"), 0.245, 1e-12);
    close(k("1"), 0.08, 1e-12);
    close(k("1.2"), 0.08 * 1.44, 1e-12);
}

#[test]
fn radial_thrust() {
    // 0,08·1000·9,80665·45·0,2·0,02
    close(
        evaluate(
            "mech.radial_thrust",
            &[
                ("kR", "0.08"),
                ("rho", "1000 kg/m3"),
                ("H", "45 m"),
                ("d2", "0.2 m"),
                ("b2tot", "0.02 m"),
            ],
        ),
        141.2158,
        1e-3,
    );
}

#[test]
fn axial_thrust_estimate() {
    // 0,8·1000·9,80665·45·π/4·(0,1² − 0,03²)
    close(
        evaluate(
            "mech.axial_thrust_estimate",
            &[
                ("f_ax", "0.8"),
                ("rho", "1000 kg/m3"),
                ("H", "45 m"),
                ("dsp", "0.1 m"),
                ("dD", "0.03 m"),
            ],
        ),
        2523.216,
        1e-3,
    );
}

#[test]
fn blade_thickness_ratio() {
    // 0,015·(7/7)^0,4·(1 + 0,25)·(1 + 0,5) con u2 = uRef y nq = nq,Ref.
    close(
        evaluate(
            "mech.blade_thickness_ratio",
            &[
                ("z", "7"),
                ("u2", "100 m/s"),
                ("u_ref", "100 m/s"),
                ("nq", "100"),
            ],
        ),
        0.028125,
        1e-12,
    );
    // 0,015·(7/6)^0,4·(1 + 0,25·0,3²)·(1 + 0,5·0,3²)
    close(
        evaluate(
            "mech.blade_thickness_ratio",
            &[
                ("z", "6"),
                ("u2", "30 m/s"),
                ("u_ref", "100 m/s"),
                ("nq", "30"),
            ],
        ),
        0.017047,
        1e-6,
    );
}

#[test]
fn shroud_thickness() {
    close(
        evaluate("mech.shroud_thickness", &[("e", "0.004 m")]),
        0.005,
        1e-12,
    );
}

#[test]
fn seal_clearance() {
    // En dsp = dRef, Δd = 0,4 mm.
    close(
        evaluate(
            "mech.seal_clearance",
            &[("dsp", "0.1 m"), ("d_ref", "0.1 m")],
        ),
        0.0004,
        1e-12,
    );
    // 0,004·250 mm·(100/250)^0,53
    close(
        evaluate(
            "mech.seal_clearance",
            &[("dsp", "0.25 m"), ("d_ref", "0.1 m")],
        ),
        0.000_615_31,
        1e-8,
    );
}

#[test]
fn driver_power() {
    close(
        evaluate("mech.driver_power", &[("P", "18 kW"), ("margin", "1.25")]),
        22500.0,
        1e-9,
    );
}

#[test]
fn disk_reynolds() {
    // ω r2² / ν con n = 1750 rpm, d2 = 0,4 m y ν = 1e-6 m²/s
    close(
        evaluate(
            "design.disk_reynolds",
            &[("n", "1750 rpm"), ("d2", "0.4 m"), ("nu", "1e-6 m2/s")],
        ),
        7.330_383e6,
        10.0,
    );
}

#[test]
fn disk_friction_coefficient() {
    // 0,0625/Re^0,2·(1 − k0)^1,75 con Re = 1e7 y k0 = 0,5
    close(
        evaluate(
            "design.disk_friction_coefficient",
            &[("Re", "1e7"), ("k0", "0.5")],
        ),
        7.397_37e-4,
        1e-8,
    );
}

#[test]
fn disk_friction_ratio() {
    // 31680·kRR·fgeo/(nq²·ψ^2,5) con kRR = 0,0008, fgeo = 1,22, nq = 16, ψ = 1
    close(
        evaluate(
            "design.disk_friction_ratio",
            &[
                ("k_rr", "0.0008"),
                ("f_geo", "1.22"),
                ("nq", "16"),
                ("psi", "1"),
            ],
        ),
        0.12078,
        1e-5,
    );
}

#[test]
fn statistical_hydraulic_efficiency() {
    // Gülich ec. 3.9.6 con Q = 0,1 m³/s y nq = 30
    close(
        evaluate(
            "design.statistical_hydraulic_efficiency",
            &[("Q", "0.1 m3/s"), ("Q_ref", "1 m3/s"), ("nq", "30")],
        ),
        0.919_964,
        1e-5,
    );
}

#[test]
fn loss_efficiency() {
    // ηh·ηv·ηm/(1 + ηh·ηv·PRR/Pu) con 0,87, 0,95, 0,985 y 0,09
    close(
        evaluate(
            "design.loss_efficiency",
            &[
                ("eta_h", "0.87"),
                ("eta_v", "0.95"),
                ("eta_m", "0.985"),
                ("disk_ratio", "0.09"),
            ],
        ),
        0.757_738,
        1e-5,
    );
}

#[test]
fn disk_friction_loss() {
    // PRR = Pu·PRR/Pu
    close(
        evaluate(
            "design.disk_friction_loss",
            &[("P_hyd", "50000 W"), ("disk_ratio", "0.09")],
        ),
        4500.0,
        1e-9,
    );
}

#[test]
fn bearing_seal_loss() {
    // Pm = P·(1 − ηm)
    close(
        evaluate(
            "design.bearing_seal_loss",
            &[("P_shaft", "70000 W"), ("eta_m", "0.987")],
        ),
        910.0,
        1e-6,
    );
}

#[test]
fn zero_head_flow_ratio() {
    // q0 = γ·tan β2/(φ·τ2) con γ = 0,82, β2 = 19,3°, φ = 0,0785 y τ2 = 1.
    close(
        evaluate(
            "design.zero_head_flow_ratio",
            &[
                ("slip", "0.82"),
                ("beta2", "19.3 deg"),
                ("phi", "0.0785"),
                ("tau", "1"),
            ],
        ),
        0.82 * 19.3_f64.to_radians().tan() / 0.0785,
        1e-9,
    );
}

#[test]
fn design_head_slope() {
    // Tangente a la recta de (q0, 0) a (1, 1): −1/(q0 − 1)
    close(
        evaluate("design.design_head_slope", &[("q0", "3.5")]),
        -0.4,
        1e-12,
    );
}

#[test]
fn curve_head_ratio() {
    // h(q) = h0 + (2(1 − h0) − S)q + (S − (1 − h0))q²: pasa por h0, por 1 en q = 1
    // y su pendiente en q = 1 es S.
    let h = |q: &str| {
        evaluate(
            "design.curve_head_ratio",
            &[("h0", "1.12"), ("slope", "-0.4"), ("q", q)],
        )
    };
    close(h("0"), 1.12, 1e-12);
    close(h("1"), 1.0, 1e-12);
    close((h("1.000001") - h("0.999999")) / 2e-6, -0.4, 1e-6);
}

#[test]
fn npsh3_inlet() {
    // (λc·c1m² + λw·(c1m² + u1²))/2g con c1m = 5,5 m/s, u1 = 13 m/s
    close(
        evaluate(
            "design.npsh3_inlet",
            &[
                ("c1m", "5.5 m/s"),
                ("u1", "13 m/s"),
                ("lambda_c", "1.1"),
                ("lambda_w", "0.2"),
                ("g", "9.80665 m/s2"),
            ],
        ),
        (1.1 * 5.5_f64.powi(2) + 0.2 * (5.5_f64.powi(2) + 13.0_f64.powi(2))) / (2.0 * 9.80665),
        1e-9,
    );
}

#[test]
fn seal_rotation_factor() {
    // k = 0,9·y^0,087, y = Re_u2^0,3·s·dsp/d2²·√(s/L), Re_u2 = u2·r2/ν
    close(
        evaluate(
            "design.seal_rotation_factor",
            &[
                ("u2", "39 m/s"),
                ("d2", "0.425 m"),
                ("nu", "1e-6 m2/s"),
                ("s", "0.25 mm"),
                ("dsp", "0.141 m"),
                ("L", "17 mm"),
            ],
        ),
        0.539_961,
        1e-5,
    );
}

#[test]
fn seal_pressure_difference() {
    // RG·H − k²·u2²/2g·(1 − dsp²/d2²)
    close(
        evaluate(
            "design.seal_pressure_difference",
            &[
                ("RG", "0.75"),
                ("H", "83 m"),
                ("k", "0.54"),
                ("u2", "39 m/s"),
                ("g", "9.80665 m/s2"),
                ("dsp", "0.141 m"),
                ("d2", "0.425 m"),
            ],
        ),
        42.125_598,
        1e-5,
    );
}

#[test]
fn seal_friction_coefficient() {
    // max(λtur, λlam)·(1 + 0,19·(usp/cax)²)^0,375 con A = 0,135·ε/s
    close(
        evaluate(
            "design.seal_friction_coefficient",
            &[
                ("s", "0.25 mm"),
                ("cax", "16 m/s"),
                ("nu", "1e-6 m2/s"),
                ("eps", "0.004 mm"),
                ("usp", "13 m/s"),
            ],
        ),
        0.050_750,
        1e-5,
    );
}

#[test]
fn seal_rotation_laminar() {
    // Gülich 3.7.14: λ/λ0 = {1 + 0,2·(Reu/2000)²}^1,03
    let factor = |reu: &str| evaluate("design.seal_rotation_laminar", &[("Reu", reu)]);
    close(factor("1000"), 1.051_538, 1e-6);
    close(factor("500"), 1.012_877, 1e-6);
    close(factor("0"), 1.0, 1e-12);
}

#[test]
fn seal_axial_velocity() {
    // √(2g·ΔH/(ζ + λ·L/2s))
    close(
        evaluate(
            "design.seal_axial_velocity",
            &[
                ("g", "9.80665 m/s2"),
                ("dH", "42 m"),
                ("zeta", "1.1"),
                ("lambda", "0.05"),
                ("L", "17 mm"),
                ("s", "0.25 mm"),
            ],
        ),
        17.152_245,
        1e-5,
    );
}

#[test]
fn seal_leakage_flow() {
    // π·dsp·s·cax
    close(
        evaluate(
            "design.seal_leakage_flow",
            &[("dsp", "0.141 m"), ("s", "0.25 mm"), ("cax", "16.8 m/s")],
        ),
        0.001_860_45,
        1e-8,
    );
}

#[test]
fn seal_volumetric_efficiency() {
    // Q/(Q + Qsp)
    close(
        evaluate(
            "design.seal_volumetric_efficiency",
            &[("Q", "0.0669 m3/s"), ("Qsp", "0.0018 m3/s")],
        ),
        0.973_799,
        1e-5,
    );
}
