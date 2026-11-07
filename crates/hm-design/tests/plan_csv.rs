//! Exportar la vista en planta: el CSV releído es el mismo modelo, bit a bit.

use hm_design::geometry::geometry;
use hm_design::plan::{PlanCurve, PlanModel, plan_model};
use hm_design::{DesignInput, design};
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

fn free_plan() -> PlanModel {
    let result = design(&input()).unwrap();
    let shape = geometry(&input(), &result).unwrap();
    plan_model(&result, &shape).unwrap()
}

/// Campos de una línea CSV con comillas a la RFC 4180, escrito aparte del
/// código que exporta.
fn fields(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                out.last_mut().unwrap().push('"');
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(String::new()),
            _ => out.last_mut().unwrap().push(c),
        }
    }
    out
}

/// Relee el archivo: devuelve la huella de la cabecera y el modelo.
fn reread(csv: &str) -> (String, PlanModel) {
    let body = csv.strip_prefix('\u{feff}').expect("BOM al principio");
    let mut lines = body.lines();
    let header = lines.next().unwrap();
    let fingerprint = header
        .strip_prefix("# hmflow vista en planta, mm, huella ")
        .expect("cabecera")
        .to_string();
    assert_eq!(lines.next(), Some("curva,punto,x_mm,y_mm"));
    let mut curves: Vec<PlanCurve> = Vec::new();
    for line in lines {
        let row = fields(line);
        assert_eq!(row.len(), 4, "{line}");
        let point = [row[2].parse().unwrap(), row[3].parse().unwrap()];
        let index: usize = row[1].parse().unwrap();
        match curves.last_mut() {
            Some(curve) if curve.name == row[0] && index == curve.points.len() => {
                curve.points.push(point);
            }
            _ => {
                assert_eq!(index, 0, "{line}");
                curves.push(PlanCurve {
                    name: row[0].clone(),
                    points: vec![point],
                });
            }
        }
    }
    (fingerprint, PlanModel { curves })
}

fn bits(model: &PlanModel) -> Vec<(String, Vec<[u64; 2]>)> {
    model
        .curves
        .iter()
        .map(|curve| {
            let points = curve
                .points
                .iter()
                .map(|p| [p[0].to_bits(), p[1].to_bits()])
                .collect();
            (curve.name.clone(), points)
        })
        .collect()
}

#[test]
fn the_reread_file_is_the_same_model_bit_for_bit() {
    let plan = free_plan();
    let (fingerprint, again) = reread(&plan.to_csv());
    assert_eq!(bits(&again), bits(&plan));
    assert_eq!(fingerprint, plan.fingerprint_text());
    assert_eq!(again.fingerprint_text(), fingerprint);
}

#[test]
fn the_file_starts_with_the_mark_and_the_two_header_lines() {
    let plan = free_plan();
    let csv = plan.to_csv();
    let mut lines = csv.lines();
    assert_eq!(
        lines.next().unwrap(),
        format!(
            "\u{feff}# hmflow vista en planta, mm, huella {}",
            plan.fingerprint_text()
        )
    );
    assert_eq!(lines.next(), Some("curva,punto,x_mm,y_mm"));
    assert!(lines.next().unwrap().starts_with("D1,0,"));
    assert!(csv.ends_with('\n'));
    let rows: usize = plan.curves.iter().map(|curve| curve.points.len()).sum();
    assert_eq!(csv.lines().count(), rows + 2);
}

#[test]
fn awkward_names_and_numbers_survive_the_trip() {
    let plan = PlanModel {
        curves: vec![
            PlanCurve {
                name: "Corte \"A\", lado 1".into(),
                points: vec![[0.1 + 0.2, -0.0], [1e-300, -123_456.789_012_345_6]],
            },
            PlanCurve {
                name: "Tramo \"B\"".into(),
                points: vec![[1.0, 2.0]],
            },
            PlanCurve {
                name: "Álabe 1".into(),
                points: vec![[f64::MIN_POSITIVE, 2.0 / 3.0]],
            },
        ],
    };
    let csv = plan.to_csv();
    assert!(csv.contains("\"Corte \"\"A\"\", lado 1\",0,"), "{csv}");
    // Sin cientos de ceros: lo muy pequeño va con exponente.
    assert!(csv.contains(",1e-300,"), "{csv}");
    assert!(csv.lines().all(|line| line.len() < 120), "{csv}");
    let (fingerprint, again) = reread(&csv);
    assert_eq!(bits(&again), bits(&plan));
    assert_eq!(fingerprint, plan.fingerprint_text());
}
