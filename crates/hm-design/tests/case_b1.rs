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

use hm_design::{
    DesignInput, DesignOptions, curve_head, default_mechanical_efficiency, design,
    design_with_options, npshr_ratio,
};
use hm_units::parse_quantity;

/// Punto de máximo rendimiento del catálogo.
const Q_DESIGN_GPM: f64 = 1060.0;
const H_DESIGN_M: f64 = 83.0;
const SPEED_RPM: f64 = 1750.0;
const D2_MM: f64 = 425.0;
const ETA_DESIGN: f64 = 0.73;

/// Curva H(Q) de catálogo con el rodete de 425 mm, leída de los trazos
/// vectoriales del PDF del catálogo (tramo continuo, de 306 a 1387 gpm).
/// La lectura es exacta; la incertidumbre es la del dibujo del fabricante.
const CATALOG_HQ: &[(f64, f64)] = &[
    (400.0, 93.6),
    (500.0, 92.8),
    (600.0, 91.8),
    (700.0, 90.4),
    (800.0, 88.7),
    (900.0, 86.7),
    (1000.0, 84.4),
    (1100.0, 81.7),
    (1200.0, 78.6),
    (1300.0, 75.2),
];

/// Grado 2B de ISO 9906:2012: tolerancia de altura.
const GRADE_2B_HEAD: f64 = 0.05;
/// Grado 2B de ISO 9906:2012: tolerancia de rendimiento.
const GRADE_2B_EFFICIENCY: f64 = 0.05;

fn input() -> DesignInput {
    let q = parse_quantity(&format!("{Q_DESIGN_GPM} gpm")).unwrap();
    let n = parse_quantity(&format!("{SPEED_RPM} rpm")).unwrap();
    DesignInput {
        h: parse_quantity(&format!("{H_DESIGN_M} m")).unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: default_mechanical_efficiency(&q, &n).unwrap(),
        npsha: None,
        q,
        n,
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

/// Geometría medida del rodete real de B1 (docs/validation/matrix.md): 6
/// álabes y b2 ≈ 17 mm, que con el φ del motor es φ = 0,0785. β2 lo resuelve
/// Euler.
///
/// La entrada: ojo de ≈ 141 mm en la tapa con cubo de ≈ 66 mm. El motor no
/// modela el cubo: el área del ojo se representa con c0 = Qth/A = 5,73 m/s
/// (D0 = 124,6 mm) y J = 141/124,6 para el diámetro en la tapa.
fn real_impeller() -> DesignOptions {
    DesignOptions {
        blade_count: Some(6),
        phi: Some(0.0785),
        c0: Some(5.73),
        j: Some(1.132),
        ..DesignOptions::default()
    }
}

/// NPSH3 del catálogo en el punto, interpolado entre las isolíneas de 3 m
/// (≈ 963 gpm) y 4 m (1280 gpm) sobre el rodete de 425 mm.
const NPSH3_CATALOG_M: f64 = 3.31;

/// Mayor desviación relativa de la curva frente al catálogo.
fn worst_head_deviation(options: &DesignOptions) -> f64 {
    let input = input();
    let result = design_with_options(&input, options).unwrap();
    CATALOG_HQ
        .iter()
        .map(|&(q_gpm, h_catalog)| {
            (curve_head(&input, &result, q_gpm / Q_DESIGN_GPM) / h_catalog - 1.0).abs()
        })
        .fold(0.0, f64::max)
}

#[test]
fn b1_real_impeller_curve_inside_grade_2b() {
    let worst = worst_head_deviation(&real_impeller());
    assert!(
        worst <= GRADE_2B_HEAD,
        "desviación máxima {:.1} %",
        100.0 * worst
    );
}

#[test]
fn b1_curve_tells_the_impellers_apart() {
    // Desde end_suction_v0.5 la pendiente de H(Q) sale de la geometría
    // (Gülich §4.3.5): el rodete real se acerca más a su catálogo que el
    // rodete por defecto del motor.
    let real = worst_head_deviation(&real_impeller());
    let default = worst_head_deviation(&DesignOptions::default());
    assert!(
        real < default,
        "rodete real {:.1} % frente a {:.1} % del rodete por defecto",
        100.0 * real,
        100.0 * default
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
fn b1_efficiency_inside_grade_2b() {
    // Desde end_suction_v0.4, η sale del análisis de pérdidas de Gülich
    // (tabla 3.10): ηh estadístico, fuga, rozamiento de discos y ηm.
    let result = design(&input()).unwrap();
    let deviation = result.eta / ETA_DESIGN - 1.0;
    assert!(
        deviation.abs() <= GRADE_2B_EFFICIENCY,
        "η {:.3} frente a {ETA_DESIGN} ({:+.1} %)",
        result.eta,
        100.0 * deviation
    );
}

#[test]
fn b1_real_impeller_eye_diameter() {
    let result = design_with_options(&input(), &real_impeller()).unwrap();
    let d1 = result.d1.si * 1000.0;
    assert!((d1 - 141.0).abs() < 1.0, "d1 {d1:.1} mm");
}

#[test]
fn b1_npsh3_band_contains_catalog() {
    // Gülich ec. 6.10 con λw = 0,1..0,3 (tabla D6.3): el catálogo cae dentro
    // de la banda con el ojo real. El motor usa el centro, λw = 0,2.
    let result = design_with_options(&input(), &real_impeller()).unwrap();
    let c1m = result.selection.c0;
    let npsh = |lambda_w: f64| {
        (hm_design::NPSH_LAMBDA_C * c1m.powi(2) + lambda_w * (c1m.powi(2) + result.u1.si.powi(2)))
            / (2.0 * 9.806_65)
    };
    assert!(
        (npsh(0.1)..=npsh(0.3)).contains(&NPSH3_CATALOG_M),
        "banda {:.2}..{:.2} m frente a {NPSH3_CATALOG_M} m",
        npsh(0.1),
        npsh(0.3)
    );
}

#[test]
fn b1_npsh3_tells_the_eyes_apart() {
    // Desde end_suction_v0.6 el NPSH3 del punto sale del ojo (ec. 6.10): el
    // ojo grande del rodete por defecto pide menos NPSH que el real.
    let real = design_with_options(&input(), &real_impeller()).unwrap();
    let default = design(&input()).unwrap();
    assert!(
        default.npshr_design.si < real.npshr_design.si - 0.3,
        "por defecto {:.2} m, real {:.2} m",
        default.npshr_design.si,
        real.npshr_design.si
    );
}

/// Isolíneas de NPSH del catálogo sobre el rodete de 425 mm: (gpm, m).
const CATALOG_NPSH: &[(f64, f64)] = &[(446.0, 1.0), (741.0, 2.0), (963.0, 3.0), (1280.0, 4.0)];

#[test]
fn b1_npsh3_shape_follows_catalog() {
    // Forma de NPSHr(Q)/NPSHr(Qd) frente a las isolíneas del catálogo. Las
    // isolíneas van cada 1 m: la lectura vale ±0,5 m, ±0,15 en la razón.
    //
    // Solo a caudal parcial. Con 1280 gpm el modelo da 1,76 frente a 1,21: la
    // subida (Q/Qsa)³ arranca en Qsa ≈ 0,99 Qd porque la limita la garganta de
    // la voluta simple del motor; B1 tiene doble voluta, de garganta
    // desconocida, y su entrada sin choque está en 1,23 Qd. Desviación
    // documentada en docs/validation/matrix.md.
    let result = design_with_options(&input(), &real_impeller()).unwrap();
    let at_design = npshr_ratio(&result, 1.0);
    let mut failures = Vec::new();
    for &(q_gpm, npsh) in CATALOG_NPSH.iter().filter(|(q, _)| *q <= Q_DESIGN_GPM) {
        let model = npshr_ratio(&result, q_gpm / Q_DESIGN_GPM) / at_design;
        let catalog = npsh / NPSH3_CATALOG_M;
        if (model - catalog).abs() > 0.15 {
            failures.push(format!("{q_gpm} gpm: {model:.2} frente a {catalog:.2}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn b1_seal_leakage_tells_the_impellers_apart() {
    // Anillo delantero en el ojo (Gülich tabla 3.7): el ojo real, más pequeño,
    // fuga menos que el grande por defecto.
    let real = design_with_options(&input(), &real_impeller()).unwrap();
    let default = design(&input()).unwrap();
    assert!(
        real.seal_eta_v > default.seal_eta_v,
        "real {:.4}, por defecto {:.4}",
        real.seal_eta_v,
        default.seal_eta_v
    );
}
