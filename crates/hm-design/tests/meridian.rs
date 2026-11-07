//! Corte meridional: cada tapa es una Bézier de 5 puntos (Gülich 2020,
//! §7.2.2.1, pp. 416-418), con P1 y P3 acoplados a su recta y P2 libre.

use std::f64::consts::PI;

use hm_design::meridian::{
    ControlPoint, MeridianEdits, MeridionalModel, Moved, Wall, WallEdit, meridional_model,
    meridional_model_with,
};
use hm_design::{DesignInput, DesignResult, design};
use hm_geometry::{Bezier, Point};
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

fn points<'a>(model: &'a MeridionalModel, name: &str) -> &'a [[f64; 2]] {
    &model.curve(name).unwrap().points
}

fn edited(edits: &MeridianEdits) -> MeridionalModel {
    meridional_model_with(&design(&input()).unwrap(), edits).unwrap()
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
    let b1g = (mm(&result.d1) - p.d1i) / 2.0;
    assert!((p.inlet_width - b1g).abs() < 1e-12);
    assert!((p.z_e - 0.75 * b1g * result.nq.powf(-0.05)).abs() < 1e-12);
    assert!((p.r_ds - 0.7 * b1g).abs() < 1e-12);
}

#[test]
fn the_ends_are_fixed_by_the_1d_design() {
    let (result, model) = model();
    let (r1, r2, b2) = (mm(&result.d1) / 2.0, mm(&result.d2) / 2.0, mm(&result.b2));
    let z_e = model.params.z_e;
    let front = points(&model, "Tapa delantera");
    let rear = points(&model, "Tapa trasera");
    assert_eq!(front[0], [0.0, r1]);
    assert_eq!(front[front.len() - 1], [z_e, r2]);
    assert_eq!(rear[0], [0.0, model.params.d1i / 2.0]);
    assert_eq!(rear[rear.len() - 1], [z_e + b2, r2]);
    assert_eq!(
        points(&model, "Borde de salida"),
        [[z_e + b2, r2], [z_e, r2]]
    );
}

#[test]
fn the_inlet_area_is_the_one_of_the_1d_design() {
    let (result, model) = model();
    let leading = points(&model, "Borde de ataque");
    let (outer, inner) = (leading[0][1], leading[1][1]);
    let annulus = PI * (outer * outer - inner * inner);
    let design = PI * mm(&result.d1) * mm(&result.b1);
    assert!((annulus - design).abs() / design < 1e-12);
}

/// P2 en la esquina de las tangentes extremas: la curva sale axial y llega
/// radial (εDs = εTs = 0), y con P1 y P3 acoplados no tiene curvatura en los
/// extremos.
#[test]
fn the_engine_puts_p2_in_the_corner_and_couples_p1_and_p3() {
    let (result, model) = model();
    let (r1, b2) = (mm(&result.d1) / 2.0, mm(&result.b2));
    let p = &model.params;
    assert_eq!(p.shroud.points[2], [p.z_e, r1]);
    assert_eq!(p.hub.points[2], [p.z_e + b2, p.d1i / 2.0]);
    for wall in Wall::ALL {
        let shape = p.wall(wall);
        let [p0, p1, p2, p3, p4] = shape.points;
        let on_line = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() < 1e-9
        };
        assert!(on_line(p0, p1, p2) && on_line(p4, p3, p2), "{wall:?}");
        let curve = Bezier::new(
            shape
                .points
                .iter()
                .map(|q| Point::new(q[0], q[1]))
                .collect(),
        )
        .unwrap();
        assert!(curve.curvature_at(0.0).unwrap().abs() < 1e-12);
        assert!(curve.curvature_at(1.0).unwrap().abs() < 1e-12);
        assert_eq!((shape.p1, shape.p3), (shape.auto_p1, shape.auto_p3));
    }
}

/// Las fracciones del motor dan la curva más suave: moverlas en cualquier
/// sentido no sube el radio mínimo.
#[test]
fn the_engine_fractions_give_the_smoothest_curve() {
    let (_, model) = model();
    for wall in Wall::ALL {
        let shape = model.params.wall(wall).clone();
        for (d1, d3) in [(0.03, 0.0), (-0.03, 0.0), (0.0, 0.03), (0.0, -0.03)] {
            let mut edits = MeridianEdits::default();
            *edits.wall_mut(wall) = WallEdit {
                p1: Some(shape.p1 + d1),
                p3: Some(shape.p3 + d3),
                p2: None,
            };
            let other = edited(&edits);
            assert!(
                other.params.wall(wall).min_radius <= shape.min_radius + 1e-6,
                "{wall:?} {d1} {d3}"
            );
        }
    }
}

#[test]
fn the_channel_narrows_from_the_inlet_to_b2_without_jumps() {
    let (result, model) = model();
    let channel = model.channel();
    let inlet = PI * mm(&result.d1) * mm(&result.b1);
    let outlet = PI * mm(&result.d2) * mm(&result.b2);
    assert!((channel[0].area - inlet).abs() / inlet < 1e-9);
    assert!((channel[channel.len() - 1].area - outlet).abs() / outlet < 1e-9);
    for pair in channel.windows(2) {
        assert!(pair[1].width <= pair[0].width + 0.01, "{pair:?}");
        assert!((pair[1].area - pair[0].area).abs() / pair[0].area < 0.03);
    }
}

#[test]
fn the_meridional_velocity_is_the_flow_through_the_area() {
    let (result, model) = model();
    let channel = model.channel();
    let expected = result.qth.si / (PI * result.d2.si * result.b2.si);
    let cm = channel[channel.len() - 1].cm(result.qth.si);
    assert!((cm - expected).abs() / expected < 1e-9);
}

#[test]
fn moving_p2_moves_p1_and_p3_with_it_and_only_that_wall() {
    let (_, auto) = model();
    let corner = auto.params.shroud.points[2];
    let mut edits = MeridianEdits::default();
    edits.shroud.p2 = Some([corner[0] - 5.0, corner[1] + 8.0]);
    let model = edited(&edits);
    let shape = &model.params.shroud;
    assert_eq!(shape.points[2], [corner[0] - 5.0, corner[1] + 8.0]);
    // P1 y P3 conservan su fracción sobre las rectas nuevas.
    let expected_p1 = [
        shape.points[0][0] + (shape.points[2][0] - shape.points[0][0]) * shape.p1,
        shape.points[0][1] + (shape.points[2][1] - shape.points[0][1]) * shape.p1,
    ];
    assert_eq!(shape.points[1], expected_p1);
    assert_eq!(
        points(&model, "Tapa trasera"),
        points(&auto, "Tapa trasera")
    );
    assert_ne!(
        points(&model, "Tapa delantera"),
        points(&auto, "Tapa delantera")
    );
}

#[test]
fn a_dragged_handle_gives_a_point_or_a_fraction() {
    let (_, model) = model();
    let handles = model.handles();
    assert_eq!(handles.len(), 6);
    let p2 = ControlPoint {
        wall: Wall::Hub,
        index: 2,
    };
    assert_eq!(model.moved(p2, [30.0, 25.0]), Moved::Point([30.0, 25.0]));
    // P1 en el punto medio de P0–P2 es la fracción 0,5.
    let [p0, _, corner, _, _] = model.params.shroud.points;
    let middle = [(p0[0] + corner[0]) / 2.0, (p0[1] + corner[1]) / 2.0 + 3.0];
    let p1 = ControlPoint {
        wall: Wall::Shroud,
        index: 1,
    };
    match model.moved(p1, middle) {
        Moved::Fraction(value) => assert!((value - 0.5).abs() < 1e-12),
        other => panic!("{other:?}"),
    }
    // Fuera de la recta, se recorta a lo admitido.
    assert_eq!(model.moved(p1, [-100.0, p0[1]]), Moved::Fraction(0.02));
}

#[test]
fn bad_edits_are_refused_by_name() {
    let mut fraction = MeridianEdits::default();
    fraction.hub.p3 = Some(1.0);
    let error = meridional_model_with(&design(&input()).unwrap(), &fraction)
        .unwrap_err()
        .to_string();
    assert!(error.contains("P3 de la tapa trasera"), "{error}");

    let mut point = MeridianEdits::default();
    point.shroud.p2 = Some([f64::NAN, 50.0]);
    let error = meridional_model_with(&design(&input()).unwrap(), &point)
        .unwrap_err()
        .to_string();
    assert!(error.contains("P2 de la tapa delantera"), "{error}");
}

#[test]
fn shrouds_that_cross_are_refused() {
    let (_, auto) = model();
    let mut edits = MeridianEdits::default();
    // P2 de la delantera muy por debajo del cubo: la curva baja y corta la trasera.
    let hub_corner = auto.params.hub.points[2];
    edits.shroud.p2 = Some([hub_corner[0], -60.0]);
    let error = meridional_model_with(&design(&input()).unwrap(), &edits)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("se cruzan") || error.contains("eje"),
        "{error}"
    );
}

#[test]
fn a_bigger_d2_changes_the_fingerprint_and_the_csv_names_the_view() {
    let (result, model) = model();
    let mut wider = result.clone();
    wider.d2 = parse_quantity("230 mm").unwrap();
    let other = meridional_model(&wider).unwrap();
    assert_ne!(model.fingerprint(), other.fingerprint());
    assert!(model.to_csv().starts_with(&format!(
        "\u{feff}# hmflow meridiano, mm, huella {}\ncurva,punto,z_mm,r_mm\n",
        model.fingerprint_text()
    )));
}

#[test]
fn an_inlet_wider_than_the_eye_is_refused() {
    let (result, _) = model();
    let mut impossible = result.clone();
    impossible.b1 = parse_quantity("40 mm").unwrap();
    let error = meridional_model(&impossible).unwrap_err();
    assert!(error.to_string().contains("b1"), "{error}");
}
