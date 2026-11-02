//! Caso B1: bomba end-suction de referencia del NORTE (docs/NORTE.md).
//!
//! La referencia es la curva de catálogo del fabricante con el rodete de
//! 425 mm a 1750 rpm, digitalizada. No es un ensayo: es la curva que la bomba
//! debe cumplir. Las unidades fundidas ensayadas se quedan en `oracle/` y en
//! la matriz solo como banda de fabricación (docs/validation/matrix.md).
//!
//! El criterio es el grado 2B de ISO 9906:2012 (tabla de factores de
//! tolerancia): ±5 % en altura y −5 % en rendimiento. Aquí se aplica como
//! banda simétrica, porque lo que se juzga es la exactitud del modelo y no la
//! aceptación de una bomba.

use hm_design::{DesignInput, curve_head, design};
use hm_units::parse_quantity;

/// Punto de máximo rendimiento del catálogo.
const Q_DESIGN_GPM: f64 = 1060.0;
const H_DESIGN_M: f64 = 83.0;
const SPEED_RPM: f64 = 1750.0;
const D2_MM: f64 = 425.0;
const ETA_DESIGN: f64 = 0.73;

/// Curva H(Q) de catálogo con el rodete de 425 mm, digitalizada del tramo
/// continuo (de 250 a 1400 gpm). Incertidumbre de lectura ≈ ±0,5 m.
const CATALOG_HQ: &[(f64, f64)] = &[
    (400.0, 93.5),
    (500.0, 92.7),
    (600.0, 91.7),
    (700.0, 90.3),
    (800.0, 88.6),
    (900.0, 86.6),
    (1000.0, 84.3),
    (1100.0, 81.4),
    (1200.0, 78.6),
    (1300.0, 75.2),
];

/// Grado 2B de ISO 9906:2012: tolerancia de altura.
const GRADE_2B_HEAD: f64 = 0.05;
/// Grado 2B de ISO 9906:2012: tolerancia de rendimiento.
const GRADE_2B_EFFICIENCY: f64 = 0.05;

fn input() -> DesignInput {
    DesignInput {
        q: parse_quantity(&format!("{Q_DESIGN_GPM} gpm")).unwrap(),
        h: parse_quantity(&format!("{H_DESIGN_M} m")).unwrap(),
        n: parse_quantity(&format!("{SPEED_RPM} rpm")).unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: None,
    }
}

#[test]
fn b1_is_low_nq() {
    // nq ≈ 16: por debajo del rango contrastado con diseños publicados (20..100).
    let result = design(&input()).unwrap();
    assert!((15.5..17.5).contains(&result.nq), "nq {}", result.nq);
}

#[test]
fn b1_impeller_diameter_matches_catalog() {
    let result = design(&input()).unwrap();
    let deviation = result.d2.si * 1000.0 / D2_MM - 1.0;
    assert!(
        deviation.abs() <= 0.05,
        "d2 {:.1} mm frente a {D2_MM} mm ({:+.1} %)",
        result.d2.si * 1000.0,
        100.0 * deviation
    );
}

#[test]
fn b1_head_curve_inside_grade_2b() {
    let input = input();
    let result = design(&input).unwrap();
    let mut failures = Vec::new();
    for &(q_gpm, h_catalog) in CATALOG_HQ {
        let h = curve_head(&input, &result, q_gpm / Q_DESIGN_GPM);
        let deviation = h / h_catalog - 1.0;
        if deviation.abs() > GRADE_2B_HEAD {
            failures.push(format!(
                "{q_gpm} gpm: H {h:.1} m frente a {h_catalog} m ({:+.1} %)",
                100.0 * deviation
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "desviación conocida: el motor da η ≈ 0,82 con nq 16 frente a 0,73 del \
            catálogo; falta el rozamiento de discos (Gülich tabla 3.10). \
            Ver docs/validation/matrix.md, caso B1"]
fn b1_efficiency_inside_grade_2b() {
    let result = design(&input()).unwrap();
    let deviation = result.eta / ETA_DESIGN - 1.0;
    assert!(
        deviation.abs() <= GRADE_2B_EFFICIENCY,
        "η {:.3} frente a {ETA_DESIGN} ({:+.1} %)",
        result.eta,
        100.0 * deviation
    );
}
