//! Contraste del motor completo con diseños publicados en varios nq.
//!
//! Cada caso corre `design` con la selección por defecto y compara los dos
//! diámetros principales del rodete con los publicados. La banda admitida de
//! cada caso está en docs/validation/matrix.md: ±5 % en d2 y ±2 % en d1
//! cuando el motor reproduce el diseño, y una banda que acota la desviación
//! cuando es conocida y está documentada. Si un cambio del motor mueve una
//! desviación fuera de su banda, hay que revisar la matriz.

use hm_design::{DesignInput, design};
use hm_units::parse_quantity;

struct Reference {
    name: &'static str,
    source: &'static str,
    q: &'static str,
    h: &'static str,
    n: &'static str,
    nq: f64,
    d2_mm: f64,
    d1_mm: f64,
    /// Desviación admitida de d2, en tanto por uno: (mínima, máxima).
    d2_band: (f64, f64),
    /// Desviación admitida de d1, en tanto por uno: (mínima, máxima).
    d1_band: (f64, f64),
}

const MATCH_D2: (f64, f64) = (-0.05, 0.05);
const MATCH_D1: (f64, f64) = (-0.02, 0.02);

const REFERENCES: &[Reference] = &[
    Reference {
        name: "D1.B multietapa, por etapa",
        source: "Srinivasan 2017, Diseño D1.B, pp. 435-438",
        q: "100 m3/h",
        h: "247 m",
        n: "7500 rpm",
        nq: 20.05,
        d2_mm: 192.0,
        d1_mm: 110.0,
        // Otro método: c2u = 0,5 u2 y u2 = 75 m/s; el motor da ψ = 1,04.
        d2_band: (-0.11, -0.07),
        d1_band: MATCH_D1,
    },
    Reference {
        name: "Gülich D7.5, multietapa",
        source: "Gülich 2020, tabla D7.5, p. 535",
        q: "105 m3/h",
        h: "58 m",
        n: "2950 rpm",
        nq: 24.0,
        d2_mm: 217.0,
        d1_mm: 101.0,
        d2_band: MATCH_D2,
        // Cubo de 48 mm y nss = 180 en el libro; el motor no modela el cubo
        // y su c0 de tabla da un ojo mayor: +12 %.
        d1_band: (0.09, 0.15),
    },
    Reference {
        name: "D1.A1 una etapa",
        source: "Srinivasan 2017, Diseño D1.A1, pp. 415-418",
        q: "0.016 m3/s",
        h: "13.8 m",
        n: "1800 rpm",
        nq: 31.8,
        d2_mm: 187.1,
        d1_mm: 78.7,
        d2_band: MATCH_D2,
        d1_band: MATCH_D1,
    },
    Reference {
        name: "Gülich D7.5, una etapa nq 36",
        source: "Gülich 2020, tabla D7.5, p. 535",
        q: "200 m3/h",
        h: "20 m",
        n: "1450 rpm",
        nq: 36.0,
        d2_mm: 281.0,
        d1_mm: 147.0,
        d2_band: MATCH_D2,
        d1_band: MATCH_D1,
    },
    Reference {
        name: "Gülich D7.5, una etapa nq 100",
        source: "Gülich 2020, tabla D7.5, p. 535 (d2a = 312 mm, borde exterior)",
        q: "954 m3/h",
        h: "15 m",
        n: "1450 rpm",
        nq: 100.0,
        d2_mm: 312.0,
        d1_mm: 229.0,
        d2_band: MATCH_D2,
        // c0 = 4,5 m/s de tabla para nq ≥ 60 da un ojo demasiado grande.
        d1_band: (0.20, 0.28),
    },
];

fn input(reference: &Reference) -> DesignInput {
    DesignInput {
        q: parse_quantity(reference.q).unwrap(),
        h: parse_quantity(reference.h).unwrap(),
        n: parse_quantity(reference.n).unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: None,
    }
}

#[test]
fn published_designs_stay_inside_their_bands() {
    let mut failures = Vec::new();
    for reference in REFERENCES {
        let result = design(&input(reference)).unwrap();
        // El punto de operación es el publicado: nq coincide.
        assert!(
            (result.nq - reference.nq).abs() < 2.5,
            "{}: nq {:.1} frente a {}",
            reference.name,
            result.nq,
            reference.nq
        );
        let d2 = result.d2.si * 1000.0 / reference.d2_mm - 1.0;
        let d1 = result.d1.si * 1000.0 / reference.d1_mm - 1.0;
        for (label, deviation, (low, high)) in
            [("d2", d2, reference.d2_band), ("d1", d1, reference.d1_band)]
        {
            if !(low..=high).contains(&deviation) {
                failures.push(format!(
                    "{} ({}): {label} {:+.1} % fuera de {:+.0}..{:+.0} %",
                    reference.name,
                    reference.source,
                    100.0 * deviation,
                    100.0 * low,
                    100.0 * high
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn references_cover_the_calibrated_range() {
    let low = REFERENCES
        .iter()
        .map(|r| r.nq)
        .fold(f64::INFINITY, f64::min);
    let high = REFERENCES.iter().map(|r| r.nq).fold(0.0, f64::max);
    assert!(low <= 20.5 && high >= 99.0, "nq {low}..{high}");
}
