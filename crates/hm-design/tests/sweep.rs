//! Barrido de puntos de operación de nq ≈ 2 a 235: el diseño sale con Euler
//! cerrado y rendimientos coherentes, o falla con una razón con nombre.
//!
//! Es la guarda de la revisión del método v0.9: un cambio que haga fallar sin
//! explicación, o que deje un diseño con η fuera de (0, 1) o sin cerrar la
//! altura, rompe este test.

use hm_design::{DesignInput, DesignOptions, default_mechanical_efficiency, design_with_options};
use hm_units::parse_quantity;

const G: f64 = 9.806_65;

/// (Q en m³/h, H en m, n en rpm). Cubren nq de ≈ 2 a ≈ 235 y caudales por
/// debajo de 0,005 m³/s (18 m³/h), el límite del ηh estadístico.
const POINTS: [(f64, f64, f64); 24] = [
    (2.0, 30.0, 2900.0),
    (4.0, 150.0, 2900.0),
    (5.0, 60.0, 2900.0),
    (8.0, 30.0, 3550.0),
    (10.0, 100.0, 2900.0),
    (15.0, 400.0, 2950.0),
    (20.0, 80.0, 2900.0),
    (30.0, 200.0, 3550.0),
    (40.0, 100.0, 2900.0),
    (50.0, 50.0, 2900.0),
    (60.0, 120.0, 2900.0),
    (80.0, 160.0, 2900.0),
    (100.0, 10.0, 3550.0),
    (120.0, 45.0, 2900.0),
    (200.0, 8.0, 1750.0),
    (300.0, 30.0, 1450.0),
    (500.0, 60.0, 1450.0),
    (800.0, 20.0, 1450.0),
    (954.0, 15.0, 1450.0),
    (1000.0, 5.0, 980.0),
    (1500.0, 15.0, 1450.0),
    (3000.0, 10.0, 1450.0),
    (3811.0, 83.0, 1750.0),
    (240.0, 90.0, 1750.0),
];

fn input(q_m3h: f64, h: f64, n_rpm: f64) -> DesignInput {
    let q = parse_quantity(&format!("{q_m3h} m3/h")).unwrap();
    let n = parse_quantity(&format!("{n_rpm} rpm")).unwrap();
    DesignInput {
        eta_m: default_mechanical_efficiency(&q, &n).unwrap_or(0.98),
        q,
        h: parse_quantity(&format!("{h} m")).unwrap(),
        n,
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        npsha: None,
        nu: None,
    }
}

#[test]
fn design_sweep_never_fails_without_a_named_reason() {
    let mut designed = 0;
    for (q, h, n) in POINTS {
        let input = input(q, h, n);
        let label = format!("{q} m3/h, {h} m, {n} rpm");
        match design_with_options(&input, &DesignOptions::default()) {
            Ok(result) => {
                designed += 1;
                // η, ηh y ηv dentro de (0, 1).
                for (name, value) in [
                    ("eta", result.eta),
                    ("eta_h", result.eta_h),
                    ("eta_v", result.eta_v),
                ] {
                    assert!(value > 0.0 && value < 1.0, "{label}: {name} = {value}");
                }
                // Euler cierra la altura pedida: H = ηh·u2·cu2/g.
                let euler_head = result.eta_h * result.u2.si * result.cu2.si / G;
                assert!(
                    (euler_head - input.h.si).abs() <= 1e-6 * input.h.si,
                    "{label}: Euler da {euler_head} m frente a {} m",
                    input.h.si
                );
                // El diseño es físico: ni NaN ni potencias negativas.
                assert!(
                    result.p_shaft.si.is_finite() && result.p_shaft.si > 0.0,
                    "{label}"
                );
                assert!(result.d1.si > 0.0 && result.d1.si < result.d2.si, "{label}");
            }
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("ηh") || text.contains("ningún β2"),
                    "{label}: error sin razón con nombre: {text}"
                );
                // Con Q < 0,005 m³/s el error nombra el rango del ηh (tarea 2).
                if input.q.si < 0.005 {
                    assert!(text.contains("0,005"), "{label}: {text}");
                }
            }
        }
    }
    // Guarda contra un barrido que falle entero: con el método v0.9 salen al
    // menos 16 de los 24 puntos.
    assert!(
        designed >= 16,
        "solo {designed} de {} diseñados",
        POINTS.len()
    );
}

#[test]
fn low_nq_points_that_used_to_fail_now_design() {
    // Con la comprobación de alcanzabilidad solo en β2 = 89° (v0.8), estos
    // puntos de nq ≈ 10 daban «ningún β2 da la altura».
    for (q, h, n) in [
        (60.0, 120.0, 2900.0),
        (80.0, 160.0, 2900.0),
        (40.0, 100.0, 2900.0),
    ] {
        design_with_options(&input(q, h, n), &DesignOptions::default())
            .unwrap_or_else(|error| panic!("{q} m3/h, {h} m, {n} rpm: {error}"));
    }
}

#[test]
fn default_blades_keep_beta2_in_range_from_nq_11() {
    // nq de ≈ 10,8 a ≈ 26,5. Z puede saltar con nq: lo que se fija es el rango.
    for q in [50.0, 60.0, 80.0, 100.0, 150.0, 200.0, 300.0] {
        let result =
            design_with_options(&input(q, 100.0, 2900.0), &DesignOptions::default()).unwrap();
        let selection = &result.selection;
        assert!(
            (5..=7).contains(&selection.blade_count),
            "{q} m3/h: Z = {}",
            selection.blade_count
        );
        assert!(
            (15.0..=45.0).contains(&selection.beta2_deg),
            "{q} m3/h: β2 = {}",
            selection.beta2_deg
        );
    }
}
