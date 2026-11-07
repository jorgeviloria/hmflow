//! Corte meridional del rodete: Gülich 2020, §7.2.2.1, pp. 416-418.

use std::f64::consts::PI;

use hm_design::meridian::{MeridionalModel, meridional_model};
use hm_design::{DesignInput, DesignResult, design};
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

fn model() -> (DesignResult, MeridionalModel) {
    let result = design(&input()).unwrap();
    let model = meridional_model(&result).unwrap();
    (result, model)
}

fn mm(quantity: &hm_units::Quantity) -> f64 {
    quantity.si * 1000.0
}

fn close(left: [f64; 2], right: [f64; 2]) {
    assert!(
        (left[0] - right[0]).abs() < 1e-9 && (left[1] - right[1]).abs() < 1e-9,
        "{left:?} frente a {right:?}"
    );
}

#[test]
fn the_section_has_both_shrouds_and_both_edges() {
    let (_, model) = model();
    let names: Vec<&str> = model
        .curves
        .iter()
        .map(|curve| curve.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Tapa delantera",
            "Tapa trasera",
            "Borde de ataque",
            "Borde de salida"
        ]
    );
}

#[test]
fn the_values_come_from_equation_7_2() {
    let (result, model) = model();
    let p = &model.params;
    // El b1 de Gülich es la anchura del canal en el borde de ataque.
    let b1g = (mm(&result.d1) - p.d1i) / 2.0;
    assert!((p.inlet_width - b1g).abs() < 1e-12);
    assert!((p.z_e - 0.75 * b1g * result.nq.powf(-0.05)).abs() < 1e-12);
    assert!((p.r_ds - 0.7 * b1g).abs() < 1e-12);
    assert!((p.g1 - 0.25 * b1g).abs() < 1e-12);
}

#[test]
fn the_front_shroud_runs_from_the_leading_edge_at_d1_to_d2() {
    let (result, model) = model();
    let (r1, r2) = (mm(&result.d1) / 2.0, mm(&result.d2) / 2.0);
    let front = &model.curve("Tapa delantera").unwrap().points;
    // z en el sentido del flujo, 0 en el borde de ataque.
    close(front[0], [0.0, r1]);
    close(*front.last().unwrap(), [model.params.z_e, r2]);
    // El tramo g1 es axial, en r = d1/2.
    let end_of_g1 = front
        .iter()
        .position(|p| (p[0] - model.params.g1).abs() < 1e-9)
        .expect("fin de g1");
    assert!(
        front[..=end_of_g1]
            .iter()
            .all(|p| (p[1] - r1).abs() < 1e-12)
    );
}

#[test]
fn the_outlet_is_b2_wide_at_d2() {
    let (result, model) = model();
    let (r2, b2) = (mm(&result.d2) / 2.0, mm(&result.b2));
    let rear = &model.curve("Tapa trasera").unwrap().points;
    close(*rear.last().unwrap(), [b2 + model.params.z_e, r2]);
    let trailing = &model.curve("Borde de salida").unwrap().points;
    close(trailing[0], [b2 + model.params.z_e, r2]);
    close(trailing[1], [model.params.z_e, r2]);
}

#[test]
fn the_inlet_area_is_the_one_of_the_1d_design() {
    let (result, model) = model();
    let leading = &model.curve("Borde de ataque").unwrap().points;
    let (outer, inner) = (leading[0][1], leading[1][1]);
    let annulus = PI * (outer * outer - inner * inner);
    let design = PI * mm(&result.d1) * mm(&result.b1);
    assert!(
        (annulus - design).abs() / design < 1e-12,
        "{annulus} frente a {design}"
    );
    assert!((2.0 * inner - model.params.d1i).abs() < 1e-12);
}

/// Distancia de un punto a una polilínea.
fn distance_to(point: [f64; 2], line: &[[f64; 2]]) -> f64 {
    line.windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let t = (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / (dx * dx + dy * dy))
                .clamp(0.0, 1.0);
            (point[0] - a[0] - t * dx).hypot(point[1] - a[1] - t * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn the_rear_shroud_is_flat_from_d1_and_never_behind_the_outlet() {
    let (result, model) = model();
    let (r1, r2, b2) = (mm(&result.d1) / 2.0, mm(&result.d2) / 2.0, mm(&result.b2));
    let rear = &model.curve("Tapa trasera").unwrap().points;
    close(rear[0], [0.0, model.params.d1i / 2.0]);
    close(*rear.last().unwrap(), [b2 + model.params.z_e, r2]);
    for point in rear {
        assert!(
            point[0] >= 0.0 && point[0] <= b2 + model.params.z_e,
            "{point:?}"
        );
        if point[1] >= r1 {
            assert_eq!(point[0], b2 + model.params.z_e, "{point:?}");
        }
    }
}

#[test]
fn the_channel_narrows_from_the_inlet_width_to_b2_without_widening() {
    let (result, model) = model();
    let b2 = mm(&result.b2);
    let front = &model.curve("Tapa delantera").unwrap().points;
    let rear = &model.curve("Tapa trasera").unwrap().points;
    let widths: Vec<f64> = front
        .iter()
        .map(|point| distance_to(*point, rear))
        .collect();
    // La trasera es una polilínea: la distancia a ella difiere de la exacta
    // en milésimas.
    assert!(
        (widths[0] - model.params.inlet_width).abs() < 1e-3,
        "{}",
        widths[0]
    );
    assert!((widths[widths.len() - 1] - b2).abs() < 1e-9);
    for pair in widths.windows(2) {
        assert!(pair[1] <= pair[0] + 0.01, "{pair:?}");
    }
    assert!(widths.iter().all(|width| *width >= b2 - 1e-9));
}

#[test]
fn the_front_shroud_has_no_kinks() {
    let (_, model) = model();
    let front = &model.curve("Tapa delantera").unwrap().points;
    for window in front.windows(3) {
        let first = [window[1][0] - window[0][0], window[1][1] - window[0][1]];
        let second = [window[2][0] - window[1][0], window[2][1] - window[1][1]];
        let cos = (first[0] * second[0] + first[1] * second[1])
            / (first[0].hypot(first[1]) * second[0].hypot(second[1]));
        assert!(cos > 0.99, "quiebro de {:.1}°", cos.acos().to_degrees());
    }
}

#[test]
fn a_bigger_d2_changes_the_fingerprint_and_the_csv_names_the_view() {
    let (result, model) = model();
    let mut wider = result.clone();
    wider.d2 = parse_quantity("230 mm").unwrap();
    let other = meridional_model(&wider).unwrap();
    assert_ne!(model.fingerprint(), other.fingerprint());
    let csv = model.to_csv();
    assert!(csv.starts_with(&format!(
        "\u{feff}# hmflow meridiano, mm, huella {}\ncurva,punto,z_mm,r_mm\n",
        model.fingerprint_text()
    )));
}

#[test]
fn an_inlet_wider_than_the_eye_is_refused() {
    let (result, _) = model();
    let mut impossible = result.clone();
    // π·d1·b1 mayor que el círculo de d1: no cabe ningún cubo.
    impossible.b1 = parse_quantity("40 mm").unwrap();
    let error = meridional_model(&impossible).unwrap_err();
    assert!(error.to_string().contains("b1"), "{error}");
}

#[test]
fn a_high_specific_speed_is_refused_with_its_reason() {
    let q = |text: &str| parse_quantity(text).unwrap();
    let wide = DesignInput {
        q: q("100 m3/h"),
        h: q("10 m"),
        ..input()
    };
    let result = design(&wide).unwrap();
    assert!(result.nq > 80.0, "{}", result.nq);
    let error = meridional_model(&result).unwrap_err().to_string();
    assert!(
        error.contains("semiaxial") && error.contains("nq ="),
        "{error}"
    );
}
