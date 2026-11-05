//! El caso ficticio se calculó aparte, sin este crate, con la ec. (2.2) de
//! Gülich, Kell e IAPWS-IF97.

use hm_bench::{BenchCase, BenchError, Level, compare, reduce};
use hm_design::CurvePoint;
use hm_units::{Quantity, parse_quantity};

const CASE: &str = include_str!("fixtures/ejemplo.toml");
const GPM: f64 = 3.785_411_784e-3 / 60.0;
const SPEED: f64 = 1750.0 / 60.0;
/// 105 psi de agua a 30 °C (995,65 kg/m³).
const EXPECTED_OLD_HEAD: f64 = 74.1450;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}"
    );
}

/// Quita del caso las líneas que empiezan por algún prefijo.
fn without(prefixes: &[&str]) -> String {
    CASE.lines()
        .filter(|line| !prefixes.iter().any(|prefix| line.starts_with(prefix)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn trial(text: &str) -> String {
    text.replace("level = \"bench\"", "level = \"trial\"")
}

#[test]
fn a_point_matches_the_hand_calculation() {
    let reduction = reduce(&BenchCase::parse(CASE).unwrap()).unwrap();
    let point = &reduction.points[0];
    assert_eq!((point.point, point.readings), (4, 3));
    close(point.q, 0.066_839_93, 1e-7);
    close(point.h, 85.144_04, 1e-4);
    close(point.p.unwrap(), 75_284.73, 0.01);
    close(point.eta.unwrap(), 0.738_216, 1e-5);
    close(point.h_spread, 0.070_025, 1e-5);

    let u = point.uncertainty.unwrap();
    close(u.q.unwrap(), 0.005_031_4, 1e-6);
    close(u.h, 0.006_359_1, 1e-6);
    close(u.p, 0.002_605_6, 1e-6);
    close(u.eta.unwrap(), 0.008_367_8, 1e-6);
    assert!(reduction.notes.iter().all(|note| !note.contains("sin ")));
}

#[test]
fn shutoff_has_head_and_power_without_efficiency_uncertainty() {
    let reduction = reduce(&BenchCase::parse(CASE).unwrap()).unwrap();
    let shutoff = &reduction.points[1];
    assert_eq!(shutoff.q, 0.0);
    close(shutoff.h, 92.189_49, 1e-4);
    close(shutoff.p.unwrap(), 36_579.11, 0.01);
    assert_eq!(shutoff.eta, Some(0.0));
    let u = shutoff.uncertainty.unwrap();
    assert_eq!(u.eta, None);
}

#[test]
fn npsh3_is_interpolated_and_never_extrapolated() {
    let reduction = reduce(&BenchCase::parse(CASE).unwrap()).unwrap();
    let npsh3 = |gpm: f64| {
        reduction
            .npsh
            .iter()
            .find(|series| (series.q_target / GPM - gpm).abs() < 0.1)
            .unwrap()
            .npsh3
    };
    close(npsh3(1060.0).unwrap(), 4.009_976, 1e-4);
    close(npsh3(850.0).unwrap(), 2.965_461, 1e-4);
    assert_eq!(npsh3(640.0), None);
    assert!(reduction.notes.iter().any(|note| note.contains("3 %")));
}

#[test]
fn absolute_suction_gives_the_same_head() {
    let gauge = reduce(&BenchCase::parse(CASE).unwrap()).unwrap();
    // −5,2 psi manométricos con 101,2 kPa son 9,4778... psi absolutos.
    let p_atm_psi = 101_200.0 / 6_894.757_293_168;
    let mut text = CASE.replace(
        "suction_reference = \"gauge\"",
        "suction_reference = \"absolute\"",
    );
    for gauge_psi in [
        -5.2, -5.1, -3.0, -5.0, -6.0, -7.0, -8.0, -9.0, -10.0, -10.5, -11.0, -11.5,
    ] {
        let formatted = format!("p_suction = \"{gauge_psi} psi\"");
        let absolute = format!("p_suction = \"{} psi\"", gauge_psi + p_atm_psi);
        text = text.replace(&formatted, &absolute);
    }
    let absolute = reduce(&BenchCase::parse(&text).unwrap()).unwrap();
    close(absolute.points[0].h, gauge.points[0].h, 1e-9);
    close(
        absolute.npsh[0].npsh3.unwrap(),
        gauge.npsh[0].npsh3.unwrap(),
        1e-9,
    );
}

/// Como las hojas viejas: Q y dos presiones, nada más.
const OLD_SHEET: &str = r#"
schema = 1
id = "EJEMPLO-U9-2025-11"
pump = "EJEMPLO"
unit = "U9"
month = "2025-11"
level = "trial"
authorization = false

[specified]
Q = "1060 gpm"
H = "83 m"
n = "1750 rpm"

[rig]
suction_reference = "gauge"

[[curve]]
point = 1
Q = "1060 gpm"
p_discharge = "100 psi"
p_suction = "-5 psi"
t_water = "30 degC"
"#;

#[test]
fn a_trial_says_what_it_could_not_correct() {
    let case = BenchCase::parse(OLD_SHEET).unwrap();
    assert_eq!(case.level, Level::Trial);
    let reduction = reduce(&case).unwrap();
    let point = &reduction.points[0];
    assert_eq!(
        (point.p, point.eta, point.n, point.uncertainty),
        (None, None, None, None)
    );
    close(point.q, 1060.0 * GPM, 1e-9);
    close(point.h, EXPECTED_OLD_HEAD, 1e-3);
    let notes = reduction.notes.join(" | ");
    for expected in [
        "altura de velocidad",
        "cotas",
        "velocidad medida",
        "par medido",
        "incertidumbre",
    ] {
        assert!(notes.contains(expected), "{notes}");
    }
}

#[test]
fn bench_level_lists_everything_missing() {
    let text = without(&[
        "authorization",
        "hmf",
        "[seal]",
        "kind = \"packing\"",
        "D_suction",
        "z_discharge",
    ]);
    let text = format!("authorization = false\n{text}");
    let Err(BenchError::Incomplete(missing)) = BenchCase::parse(&text) else {
        panic!("debía fallar");
    };
    let all = missing.join(" | ");
    for expected in ["authorization", "hmf", "seal", "diámetros", "alturas"] {
        assert!(all.contains(expected), "{all}");
    }
}

#[test]
fn bench_level_needs_a_series_at_qd() {
    let text = CASE.replace("Q = \"1060 gpm\"\nH", "Q = \"1200 gpm\"\nH");
    let Err(BenchError::Incomplete(missing)) = BenchCase::parse(&text) else {
        panic!("debía fallar");
    };
    assert_eq!(missing, ["una serie de NPSH en Qd"]);
}

#[test]
fn identifying_data_cannot_enter_a_case() {
    for extra in ["serial = \"000000\"", "client = \"x\"", "inspector = \"x\""] {
        let text = format!("{extra}\n{CASE}");
        assert!(
            matches!(BenchCase::parse(&text), Err(BenchError::Toml(_))),
            "{extra}"
        );
    }
    let exact_date = CASE.replace("month = \"2027-01\"", "month = \"2027-01-15\"");
    assert!(matches!(
        BenchCase::parse(&exact_date),
        Err(BenchError::Field { .. })
    ));
}

#[test]
fn bad_values_name_their_field() {
    let cases = [
        ("Q = \"1078 gpm\"", "Q = \"1078 m\"", "curve[4].Q"),
        ("T = \"425 N*m\"", "T = \"-425 N*m\"", "curve[4].T"),
        (
            "t_water = \"29.4 degC\"",
            "t_water = \"10 m\"",
            "curve[4].t_water",
        ),
        ("u = \"0.5 %\"", "u = \"mucho %\"", "instruments.flow"),
        (
            "D_suction = \"128.2 mm\"",
            "D_suction = \"0 mm\"",
            "rig.D_suction",
        ),
    ];
    for (from, to, field) in cases {
        let error = BenchCase::parse(&CASE.replace(from, to)).unwrap_err();
        assert!(error.to_string().starts_with(field), "{error}");
    }
    assert_eq!(
        BenchCase::parse(&CASE.replace("schema = 1", "schema = 2")),
        Err(BenchError::Schema(2))
    );
    let boiling = reduce(&BenchCase::parse(&CASE.replace("29.4 degC", "400 degC")).unwrap());
    assert!(matches!(boiling, Err(BenchError::Formula(_))));
}

#[test]
fn npsh_needs_atmospheric_pressure() {
    let text = trial(&without(&["p_atm"]));
    let error = BenchCase::parse(&text).unwrap_err();
    assert!(error.to_string().starts_with("rig.p_atm"), "{error}");
}

fn point(q_gpm: f64, h: f64, eta: f64, p_kw: f64, npshr: f64) -> CurvePoint {
    let quantity = |text: String| -> Quantity { parse_quantity(&text).unwrap() };
    CurvePoint {
        q: quantity(format!("{q_gpm} gpm")),
        h: quantity(format!("{h} m")),
        eta,
        p_shaft: quantity(format!("{p_kw} kW")),
        npshr: quantity(format!("{npshr} m")),
    }
}

#[test]
fn comparison_interpolates_and_leaves_out_what_it_cannot_see() {
    let reduction = reduce(&BenchCase::parse(CASE).unwrap()).unwrap();
    let curve = [
        point(800.0, 90.0, 0.70, 70.0, 2.0),
        point(1200.0, 80.0, 0.74, 80.0, 6.0),
    ];
    let comparison = compare(&reduction, &curve, SPEED).unwrap();

    let at = comparison.points.first().unwrap();
    assert_eq!(at.point, 4);
    let q_gpm = 0.066_839_93 / GPM;
    let t = (q_gpm - 800.0) / 400.0;
    close(at.h_model, 90.0 - 10.0 * t, 1e-3);
    close(at.h_deviation, at.h_model / 85.144_04 - 1.0, 1e-5);
    assert!(at.in_band);
    assert_eq!(comparison.outside, [8]);

    // La serie de 640 gpm no tiene NPSH3; las otras dos sí.
    assert_eq!(comparison.npsh.len(), 2);
    close(comparison.npsh[0].npsh3, 4.009_976, 1e-4);
}

#[test]
fn comparison_flags_a_point_outside_the_band() {
    let reduction = reduce(&BenchCase::parse(CASE).unwrap()).unwrap();
    let curve = [
        point(800.0, 110.0, 0.74, 70.0, 2.0),
        point(1200.0, 100.0, 0.74, 80.0, 6.0),
    ];
    assert!(!compare(&reduction, &curve, SPEED).unwrap().points[0].in_band);
}

#[test]
fn comparison_refuses_a_model_at_another_speed() {
    let reduction = reduce(&BenchCase::parse(CASE).unwrap()).unwrap();
    let curve = [
        point(800.0, 90.0, 0.70, 70.0, 2.0),
        point(1200.0, 80.0, 0.74, 80.0, 6.0),
    ];
    assert!(matches!(
        compare(&reduction, &curve, 2900.0 / 60.0),
        Err(BenchError::Speed { .. })
    ));
}
