//! Corte meridional: cada tapa es una Bézier de 5 puntos (Gülich 2020,
//! §7.2.2.1, pp. 416-418). P1 y P3 salen de los extremos con un ángulo y una
//! longitud propios y P2 se mueve dentro del rectángulo de los extremos.

use std::f64::consts::PI;

use hm_design::meridian::{
    ControlPoint, MIN_ARM, MeridianEdits, MeridionalModel, Moved, Wall, WallEdit, meridional_model,
    meridional_model_with,
};
use hm_design::{DesignInput, DesignResult, design};
use hm_geometry::{Bezier, Curve, Point};
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

/// P2 en la esquina de las tangentes extremas: la curva sale axial (0°) y llega
/// radial (90°) (εDs = εTs = 0). Con P2 en la esquina, esos brazos quedan sobre
/// las rectas a P2 y la curva no tiene curvatura en los extremos.
#[test]
fn the_engine_puts_p2_in_the_corner_with_an_axial_start_and_a_radial_end() {
    let (result, model) = model();
    let (r1, b2) = (mm(&result.d1) / 2.0, mm(&result.b2));
    let p = &model.params;
    assert_eq!(p.shroud.points[2], [p.z_e, r1]);
    assert_eq!(p.hub.points[2], [p.z_e + b2, p.d1i / 2.0]);
    for wall in Wall::ALL {
        let shape = p.wall(wall);
        assert_eq!((shape.start_angle, shape.end_angle), (0.0, 90.0));
        assert_eq!(
            (shape.start_length, shape.end_length),
            (shape.auto_start_length, shape.auto_end_length)
        );
        let [p0, p1, p2, p3, p4] = shape.points;
        let on_line = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() < 1e-9
        };
        assert!(on_line(p0, p1, p2) && on_line(p4, p3, p2), "{wall:?}");
        assert!(curve_of(shape).curvature_at(0.0).unwrap().abs() < 1e-9);
        assert!(curve_of(shape).curvature_at(1.0).unwrap().abs() < 1e-9);
    }
}

fn curve_of(shape: &hm_design::meridian::WallShape) -> Bezier {
    Bezier::new(
        shape
            .points
            .iter()
            .map(|q| Point::new(q[0], q[1]))
            .collect(),
    )
    .unwrap()
}

/// Las longitudes del motor dan la curva más suave: alargar o acortar un brazo
/// no sube el radio mínimo.
#[test]
fn the_engine_arm_lengths_give_the_smoothest_curve() {
    let (_, model) = model();
    for wall in Wall::ALL {
        let shape = model.params.wall(wall).clone();
        for (d0, d4) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let mut edits = MeridianEdits::default();
            *edits.wall_mut(wall) = WallEdit {
                start_length: Some(shape.start_length + d0),
                end_length: Some(shape.end_length + d4),
                ..WallEdit::default()
            };
            let other = edited(&edits);
            assert!(
                other.params.wall(wall).min_radius <= shape.min_radius + 1e-6,
                "{wall:?} {d0} {d4}"
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

/// Mover P2 ya no arrastra a P1 ni a P3: salen del extremo con su ángulo y su
/// longitud, y solo cambia la tapa editada.
#[test]
fn moving_p2_leaves_p1_and_p3_where_they_were_and_only_that_wall() {
    let (_, auto) = model();
    let corner = auto.params.shroud.points[2];
    let target = [corner[0] - 5.0, corner[1] + 8.0];
    let mut edits = MeridianEdits::default();
    edits.shroud.p2 = Some(target);
    let model = edited(&edits);
    let (shape, before) = (&model.params.shroud, &auto.params.shroud);
    assert_eq!(shape.points[2], target);
    assert_eq!(shape.points[1], before.points[1]);
    for axis in 0..2 {
        assert!((shape.points[3][axis] - before.points[3][axis]).abs() < 1e-9);
    }
    assert_eq!(
        points(&model, "Tapa trasera"),
        points(&auto, "Tapa trasera")
    );
    assert_ne!(
        points(&model, "Tapa delantera"),
        points(&auto, "Tapa delantera")
    );
}

/// Los brazos salen del extremo con el ángulo medido desde +z hacia +r; el de
/// llegada es el sentido de avance en P4.
#[test]
fn the_arms_leave_the_ends_at_the_edited_angle_and_length() {
    let mut edits = MeridianEdits::default();
    edits.shroud.start_angle = Some(30.0);
    edits.shroud.start_length = Some(10.0);
    edits.shroud.end_angle = Some(60.0);
    edits.shroud.end_length = Some(4.0);
    let shape = edited(&edits).params.shroud;
    let [p0, p1, _, p3, p4] = shape.points;
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(close(p1[0] - p0[0], 10.0 * 30f64.to_radians().cos()));
    assert!(close(p1[1] - p0[1], 10.0 * 30f64.to_radians().sin()));
    assert!(close(p4[0] - p3[0], 4.0 * 60f64.to_radians().cos()));
    assert!(close(p4[1] - p3[1], 4.0 * 60f64.to_radians().sin()));
    assert_eq!((shape.start_angle, shape.end_length), (30.0, 4.0));
}

/// Con los brazos desacoplados la curvatura en los extremos ya no es nula, y
/// el radio mínimo la cuenta.
#[test]
fn a_tilted_arm_gives_curvature_at_the_end_and_the_minimum_radius_counts_it() {
    let (_, auto) = model();
    let mut edits = MeridianEdits::default();
    edits.shroud.start_angle = Some(40.0);
    let shape = edited(&edits).params.shroud;
    let curvature = curve_of(&shape).curvature_at(0.0).unwrap().abs();
    assert!(curvature > 1e-4, "{curvature}");
    assert!(shape.min_radius <= 1.0 / curvature + 1e-9);
    assert!(shape.min_radius < auto.params.shroud.min_radius);
}

#[test]
fn p2_has_a_ceiling_at_the_rectangle_of_the_ends() {
    let (result, model) = model();
    let (r1, r2) = (mm(&result.d1) / 2.0, mm(&result.d2) / 2.0);
    let shroud = &model.params.shroud;
    assert_eq!(shroud.p2_range, ([0.0, r1], [model.params.z_e, r2]));
    let hub = &model.params.hub;
    assert_eq!(
        hub.p2_range,
        (
            [0.0, model.params.d1i / 2.0],
            [model.params.z_e + mm(&result.b2), r2]
        )
    );
    // En el borde se admite; un milímetro fuera, no.
    let mut edge = MeridianEdits::default();
    edge.shroud.p2 = Some([0.0, r2]);
    assert!(meridional_model_with(&design(&input()).unwrap(), &edge).is_ok());
    for outside in [
        [-1.0, r1 + 5.0],
        [model.params.z_e + 1.0, r1 + 5.0],
        [5.0, r1 - 1.0],
        [5.0, r2 + 1.0],
    ] {
        let mut edits = MeridianEdits::default();
        edits.shroud.p2 = Some(outside);
        let error = meridional_model_with(&design(&input()).unwrap(), &edits)
            .unwrap_err()
            .to_string();
        assert!(error.contains("P2 de la tapa delantera"), "{error}");
        assert!(error.contains("rectángulo"), "{error}");
    }
}

#[test]
fn a_dragged_handle_gives_a_point_or_an_arm() {
    let (_, model) = model();
    let handles = model.handles();
    assert_eq!(handles.len(), 6);
    let [p0, _, _, _, p4] = model.params.shroud.points;
    let control = |wall, index| ControlPoint { wall, index };
    // P2 va donde el puntero, recortado al rectángulo.
    let z_e = model.params.z_e;
    assert_eq!(
        model.moved(control(Wall::Hub, 2), [30.0, 25.0]),
        Moved::Point([30.0, 25.0])
    );
    assert_eq!(
        model.moved(control(Wall::Shroud, 2), [z_e + 50.0, p0[1] - 50.0]),
        // Recortado al rectángulo y ajustado a 0,01 mm hacia dentro.
        Moved::Point([
            (z_e * 100.0).floor() / 100.0,
            (p0[1] * 100.0).ceil() / 100.0
        ])
    );
    // P1 a 45° y 10 mm de P0.
    let s = 10.0 / 2f64.sqrt();
    match model.moved(control(Wall::Shroud, 1), [p0[0] + s, p0[1] + s]) {
        Moved::Arm { angle, length } => {
            assert!((angle - 45.0).abs() < 1e-9 && (length - 10.0).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
    // P3 arriba de P4 a 5 mm: se llega hacia +r, 90°, pasando por P3 bajo P4.
    match model.moved(control(Wall::Shroud, 3), [p4[0], p4[1] - 5.0]) {
        Moved::Arm { angle, length } => {
            assert!((angle - 90.0).abs() < 1e-9 && (length - 5.0).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
    // Hacia atrás se recorta a ±90°; encima del extremo, a la longitud mínima.
    match model.moved(control(Wall::Shroud, 1), [p0[0] - 10.0, p0[1]]) {
        Moved::Arm { angle, .. } => assert_eq!(angle, 90.0),
        other => panic!("{other:?}"),
    }
    match model.moved(control(Wall::Shroud, 1), p0) {
        Moved::Arm { length, .. } => assert_eq!(length, MIN_ARM),
        other => panic!("{other:?}"),
    }
}

#[test]
fn bad_edits_are_refused_by_name() {
    for (edit, name) in [
        (
            WallEdit {
                end_length: Some(0.0),
                ..WallEdit::default()
            },
            "longitud final de la tapa trasera",
        ),
        (
            WallEdit {
                start_length: Some(f64::NAN),
                ..WallEdit::default()
            },
            "longitud de inicio de la tapa trasera",
        ),
        (
            WallEdit {
                start_angle: Some(91.0),
                ..WallEdit::default()
            },
            "ángulo de inicio de la tapa trasera",
        ),
        (
            WallEdit {
                end_angle: Some(-91.0),
                ..WallEdit::default()
            },
            "ángulo final de la tapa trasera",
        ),
    ] {
        let edits = MeridianEdits {
            hub: edit,
            ..MeridianEdits::default()
        };
        let error = meridional_model_with(&design(&input()).unwrap(), &edits)
            .unwrap_err()
            .to_string();
        assert!(error.contains(name), "{name}: {error}");
    }

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
    // P2 de la trasera arriba y atrás, dentro de su rectángulo: la trasera sube
    // por encima de la delantera y la corta.
    let r2 = auto.params.hub.p2_range.1[1];
    edits.hub.p2 = Some([0.0, r2]);
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

/// Por defecto el borde de ataque es la radial del plano de entrada y su área
/// es la del 1D; con los dos extremos en 0 el cubo arranca en (0, dH/2).
#[test]
fn by_default_the_leading_edge_is_the_radial_with_the_inlet_area_of_the_1d() {
    let (result, auto) = model();
    let p = &auto.params;
    let one_d = PI * mm(&result.d1) * mm(&result.b1);
    assert!((p.edge_area - one_d).abs() < 1e-6 * one_d);
    assert_eq!((p.edge_shroud, p.edge_hub, p.edge_angle), (0.0, 0.0, 0.0));
    assert_eq!(p.d1i, p.auto_d1i);
    let edge = points(&auto, "Borde de ataque");
    assert_eq!(edge[0][0], 0.0);
    assert_eq!(edge[1], [0.0, p.d1i / 2.0]);
    assert_eq!(points(&auto, "Tapa trasera")[0], edge[1]);
}

/// dH es editable, también 0: el cubo arranca en el eje con el brazo axial.
#[test]
fn the_hub_diameter_is_editable_and_zero_puts_the_hub_on_the_axis() {
    let (_, auto) = model();
    let edits = MeridianEdits {
        hub_diameter: Some(0.0),
        ..MeridianEdits::default()
    };
    let axis = edited(&edits);
    assert_eq!(axis.params.d1i, 0.0);
    assert_eq!(axis.params.auto_d1i, auto.params.auto_d1i);
    let hub = points(&axis, "Tapa trasera");
    assert_eq!(hub[0], [0.0, 0.0]);
    assert!(hub.iter().all(|point| point[1] >= -1e-9));
    // La tapa delantera no cambia.
    assert_eq!(
        points(&axis, "Tapa delantera"),
        points(&auto, "Tapa delantera")
    );

    for bad in [-1.0, f64::NAN, 1e6] {
        let edits = MeridianEdits {
            hub_diameter: Some(bad),
            ..MeridianEdits::default()
        };
        let error = meridional_model_with(&design(&input()).unwrap(), &edits).unwrap_err();
        assert!(error.to_string().contains("dH"), "{error}");
    }
}

/// El borde se apoya sobre las dos curvas, que siguen aguas arriba de él; el
/// canal parte de él y el área es la de la recta real.
#[test]
fn the_leading_edge_rests_on_both_curves_and_the_channel_starts_there() {
    let (_, auto) = model();
    let edits = MeridianEdits {
        edge_shroud: Some(0.25),
        edge_hub: Some(0.4),
        ..MeridianEdits::default()
    };
    let moved = edited(&edits);
    let p = &moved.params;
    assert_eq!((p.edge_shroud, p.edge_hub), (0.25, 0.4));
    // Las curvas son las mismas: solo se mueve el borde.
    for name in ["Tapa delantera", "Tapa trasera"] {
        assert_eq!(points(&moved, name), points(&auto, name), "{name}");
    }
    // Cada extremo está sobre su curva.
    for (name, end) in [
        ("Tapa delantera", p.edge_points[0]),
        ("Tapa trasera", p.edge_points[1]),
    ] {
        let near = points(&moved, name)
            .windows(2)
            .map(|pair| {
                let (a, b) = (pair[0], pair[1]);
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let t = (((end[0] - a[0]) * dx + (end[1] - a[1]) * dy) / (dx * dx + dy * dy))
                    .clamp(0.0, 1.0);
                (end[0] - a[0] - t * dx).hypot(end[1] - a[1] - t * dy)
            })
            .fold(f64::MAX, f64::min);
        assert!(near < 1e-9, "{name}: {near}");
    }
    assert_eq!(points(&moved, "Borde de ataque"), p.edge_points);
    let length = (p.edge_points[0][0] - p.edge_points[1][0])
        .hypot(p.edge_points[0][1] - p.edge_points[1][1]);
    assert!((p.edge_length - length).abs() < 1e-12);
    let area = PI * (p.edge_points[0][1] + p.edge_points[1][1]) * length;
    assert!((p.edge_area - area).abs() < 1e-9 * area);
    // El canal arranca en el borde y su primera área es la del borde.
    let channel = moved.channel();
    let first = &channel[0];
    assert_eq!(first.shroud_point, p.edge_points[0]);
    assert_eq!(first.hub_point, p.edge_points[1]);
    assert!((first.area - p.edge_area).abs() < 1e-9 * area);
    assert!(channel.len() < auto.channel().len());
}

#[test]
fn a_leading_edge_position_out_of_range_is_an_error() {
    for (shroud, hub) in [
        (Some(-0.1), None),
        (Some(0.95), None),
        (None, Some(f64::NAN)),
        (None, Some(1.0)),
    ] {
        let edits = MeridianEdits {
            edge_shroud: shroud,
            edge_hub: hub,
            ..MeridianEdits::default()
        };
        let error = meridional_model_with(&design(&input()).unwrap(), &edits).unwrap_err();
        assert!(error.to_string().contains("borde de ataque"), "{error}");
    }
}

/// La marca de curvatura máxima está en la curva y donde la curvatura da el
/// radio mínimo de la cabecera.
#[test]
fn the_maximum_curvature_mark_is_on_the_wall_at_the_minimum_radius() {
    let (_, model) = model();
    for wall in Wall::ALL {
        let shape = model.params.wall(wall);
        let curve = curve_of(shape);
        let [z, r] = shape.max_curvature_at;
        let nearest = (0..=2000)
            .map(|k| curve.point_at(f64::from(k) / 2000.0).unwrap())
            .map(|p| (p.x - z).hypot(p.y - r))
            .fold(f64::MAX, f64::min);
        assert!(nearest < 1e-6, "{wall:?}: {nearest}");
        let t = (0..=2000)
            .map(|k| f64::from(k) / 2000.0)
            .min_by(|a, b| {
                let radius = |t: f64| 1.0 / curve.curvature_at(t).unwrap().abs();
                radius(*a).total_cmp(&radius(*b))
            })
            .unwrap();
        let there = curve.point_at(t).unwrap();
        assert!((there.x - z).hypot(there.y - r) < 0.05, "{wall:?}");
    }
}

/// Los extremos del borde se agarran y arrastran sobre sus curvas: lo que se
/// escribe es la fracción de longitud del punto de la curva más cercano.
#[test]
fn the_leading_edge_ends_are_draggable_along_their_curves() {
    let edits = MeridianEdits {
        edge_shroud: Some(0.25),
        edge_hub: Some(0.5),
        ..MeridianEdits::default()
    };
    let model = edited(&edits);
    let handles = model.edge_handles();
    assert_eq!(handles.len(), 2);
    for (handle, wall) in handles.iter().zip(Wall::ALL) {
        assert_eq!(handle.control, ControlPoint { wall, index: 0 });
        assert_eq!(handle.at, model.params.edge_points[wall as usize]);
        assert_eq!(model.control_position(handle.control), Some(handle.at));
    }
    // Llevar el puntero al propio extremo da su fracción, aunque se desvíe un poco.
    for (handle, expected) in handles.iter().zip([0.25, 0.5]) {
        let near = [handle.at[0] + 0.001, handle.at[1] - 0.001];
        let Moved::Fraction(fraction) = model.moved(handle.control, near) else {
            panic!("se esperaba una fracción");
        };
        assert!((fraction - expected).abs() < 0.01, "{fraction}");
    }
    // Más allá del final de la curva se recorta al tope.
    let far = [1.0e4, 1.0e4];
    assert_eq!(model.moved(handles[0].control, far), Moved::Fraction(0.9));
    assert_eq!(
        model.moved(handles[1].control, [-1.0e4, -1.0e4]),
        Moved::Fraction(0.0)
    );
    // Un punto de Bézier sigue sin ser del borde.
    assert!(
        model
            .control_position(ControlPoint {
                wall: Wall::Hub,
                index: 2
            })
            .is_some()
    );
}

/// El perfil de curvatura va de 0 a 1 en M/Mmax y su máximo en valor absoluto
/// es la inversa del radio mínimo de la cabecera.
#[test]
fn the_curvature_profile_spans_the_wall_and_peaks_at_the_minimum_radius() {
    let (_, model) = model();
    for wall in Wall::ALL {
        let shape = model.params.wall(wall);
        let profile = shape.curvature_profile(400).unwrap();
        assert_eq!(profile.first().unwrap()[0], 0.0);
        assert!((profile.last().unwrap()[0] - 1.0).abs() < 1e-12);
        assert!(profile.windows(2).all(|pair| pair[1][0] >= pair[0][0]));
        let peak = profile.iter().map(|row| row[1].abs()).fold(0.0, f64::max);
        assert!(
            (1.0 / peak - shape.min_radius).abs() < 1e-6 * shape.min_radius,
            "{wall:?}: {} frente a {}",
            1.0 / peak,
            shape.min_radius
        );
    }
}

/// Arrastrar P2 a cualquier sitio y escribirlo con 0,01 mm (como hace la
/// interfaz) no debe sacarlo del rectángulo: antes 41,646 se escribía «41,65».
#[test]
fn a_dragged_p2_survives_rounding_to_hundredths_of_a_millimetre() {
    let (_, model) = model();
    for wall in [Wall::Hub, Wall::Shroud] {
        let control = ControlPoint { wall, index: 2 };
        for pointer in [
            [1e4, -1e4],
            [1e4, 1e4],
            [-1e4, 1e4],
            [-1e4, -1e4],
            [30.123, 25.987],
        ] {
            let Moved::Point([z, r]) = model.moved(control, pointer) else {
                panic!("P2 debe dar un punto");
            };
            let text = |v: f64| format!("{v:.2}").parse::<f64>().unwrap();
            let mut edits = MeridianEdits::default();
            let edit = WallEdit {
                p2: Some([text(z), text(r)]),
                ..WallEdit::default()
            };
            match wall {
                Wall::Hub => edits.hub = edit,
                Wall::Shroud => edits.shroud = edit,
            }
            let design = design(&input()).unwrap();
            // Otras validaciones (p. ej. que las tapas se crucen) pueden
            // saltar con un P2 extremo; aquí solo importa que el redondeo no
            // lo saque del rectángulo.
            if let Err(error) = meridional_model_with(&design, &edits) {
                assert!(
                    !error.to_string().contains("fuera del rectángulo"),
                    "{wall:?} {pointer:?} → ({z}, {r}): {error}"
                );
            }
        }
    }
}

/// Sin abombado el borde es la recta de siempre, de dos puntos.
#[test]
fn a_zero_bow_leaves_the_edge_straight() {
    let (_, auto) = model();
    let zero = edited(&MeridianEdits {
        edge_bow_shroud: Some(0.0),
        edge_bow_hub: Some(0.0),
        ..MeridianEdits::default()
    });
    assert_eq!(points(&zero, "Borde de ataque").len(), 2);
    assert_eq!(zero.params.edge_bow, [0.0, 0.0]);
    assert_eq!(zero.params.edge_area, auto.params.edge_area);
    assert_eq!(zero.params.edge_length, auto.params.edge_length);
}

/// Con el mismo abombado en P1 y P2 el punto medio de la Bézier se desplaza
/// 0,75·b; positivo es aguas arriba (−z en un borde radial), los extremos no se
/// mueven y la curva es más larga que la cuerda.
#[test]
fn a_bowed_edge_is_a_cubic_that_keeps_its_ends_and_bulges_upstream() {
    let (_, auto) = model();
    let bow = 4.0;
    let bowed = edited(&MeridianEdits {
        edge_bow_shroud: Some(bow),
        edge_bow_hub: Some(bow),
        ..MeridianEdits::default()
    });
    let line = points(&bowed, "Borde de ataque");
    assert!(line.len() > 2);
    assert_eq!(line[0], bowed.params.edge_points[0]);
    assert_eq!(line[line.len() - 1], bowed.params.edge_points[1]);
    assert_eq!(bowed.params.edge_points, auto.params.edge_points);
    assert_eq!(bowed.params.edge_bow, [bow, bow]);
    // Borde radial en z = 0: el punto medio queda 0,75·bow aguas arriba, a
    // media altura entre los extremos.
    let ends = bowed.params.edge_points;
    let middle = line[line.len() / 2];
    assert!(
        (middle[0] - ends[0][0] + 0.75 * bow).abs() < 1e-9,
        "{middle:?}"
    );
    assert!((middle[1] - (ends[0][1] + ends[1][1]) / 2.0).abs() < 1e-9);
    // Más larga que la cuerda; el área sale de la curva, no de la cuerda.
    let polyline: f64 = line
        .windows(2)
        .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
        .sum();
    assert!((bowed.params.edge_length - polyline).abs() < 1e-12);
    assert!(bowed.params.edge_length > auto.params.edge_length);
    let area: f64 = line
        .windows(2)
        .map(|pair| {
            PI * (pair[0][1] + pair[1][1])
                * (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1])
        })
        .sum();
    assert!((bowed.params.edge_area - area).abs() < 1e-9 * area);
    assert!(bowed.params.edge_area > auto.params.edge_area);
    // Un abombado negativo va hacia aguas abajo.
    let other = edited(&MeridianEdits {
        edge_bow_shroud: Some(-bow),
        edge_bow_hub: Some(-bow),
        ..MeridianEdits::default()
    });
    let line = points(&other, "Borde de ataque");
    assert!((line[line.len() / 2][0] - 0.75 * bow).abs() < 1e-9);
}

#[test]
fn a_bow_that_is_not_a_number_is_an_error() {
    for (shroud, hub) in [(Some(f64::NAN), None), (None, Some(f64::INFINITY))] {
        let edits = MeridianEdits {
            edge_bow_shroud: shroud,
            edge_bow_hub: hub,
            ..MeridianEdits::default()
        };
        let error = meridional_model_with(&design(&input()).unwrap(), &edits)
            .unwrap_err()
            .to_string();
        assert!(error.contains("abombado"), "{error}");
    }
}
