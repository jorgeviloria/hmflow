//! Corte meridional: cada tapa es una Bézier de 5 puntos (Gülich 2020,
//! §7.2.2.1, pp. 416-418). P1 y P3 salen de los extremos con un ángulo y una
//! longitud propios y P2 se mueve dentro del rectángulo de los extremos.

use std::f64::consts::PI;

use hm_design::DesignError;
use hm_design::meridian::{
    CUT_NAME, ControlPoint, CutMode, EdgeEdit, HubCutEdit, MIN_ARM, MeridianEdits, MeridionalModel,
    Moved, Wall, WallEdit, meridional_model, meridional_model_with,
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
    // b1g sale del dH del 1D, no del dH editado (0 por defecto).
    let b1g = (mm(&result.d1) - p.auto_d1i) / 2.0;
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
    let (result, base) = model();
    // Con el dH del 1D (el cubo ya no arranca ahí por defecto).
    let model = edited(&MeridianEdits {
        hub_diameter: Some(base.params.auto_d1i),
        ..MeridianEdits::default()
    });
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
    let (result, base) = model();
    // Con el dH del 1D, el área de entrada es la del 1D.
    let model = edited(&MeridianEdits {
        hub_diameter: Some(base.params.auto_d1i),
        ..MeridianEdits::default()
    });
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
    // Con el dH del 1D: con el cubo en el eje, este P2 ya no llega a cortarla.
    let mut edits = MeridianEdits {
        hub_diameter: Some(auto.params.auto_d1i),
        ..MeridianEdits::default()
    };
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

/// Por defecto el borde de ataque es la radial del plano de entrada; con dH = 0
/// va del eje a la tapa y su área es π·r1·r1. Con el dH del 1D es el área del 1D.
#[test]
fn by_default_the_leading_edge_is_the_radial_from_the_axis_and_with_the_1d_hub_it_has_the_1d_area()
{
    let (result, auto) = model();
    let p = &auto.params;
    let r1 = mm(&result.d1) / 2.0;
    let axis_area = PI * r1 * r1;
    assert!((p.edge_area - axis_area).abs() < 1e-6 * axis_area);
    assert_eq!((p.edge_shroud, p.edge_hub, p.edge_angle), (0.0, 0.0, 0.0));
    assert_eq!(p.d1i, 0.0);
    let edge = points(&auto, "Borde de ataque");
    assert_eq!(edge[0], [0.0, r1]);
    assert_eq!(edge[1], [0.0, 0.0]);
    assert_eq!(points(&auto, "Tapa trasera")[0], edge[1]);

    let old = edited(&MeridianEdits {
        hub_diameter: Some(p.auto_d1i),
        ..MeridianEdits::default()
    });
    let one_d = PI * mm(&result.d1) * mm(&result.b1);
    assert!((old.params.edge_area - one_d).abs() < 1e-6 * one_d);
    assert_eq!(old.params.d1i, old.params.auto_d1i);
    let edge = points(&old, "Borde de ataque");
    assert_eq!(edge[1], [0.0, old.params.d1i / 2.0]);
    assert_eq!(points(&old, "Tapa trasera")[0], edge[1]);
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
        // El mínimo puede estar repetido (el ajuste suave iguala dos picos de
        // curvatura, como en el cubo sobre el eje): se comprueba que la marca
        // da el radio mínimo y que ningún punto de la curva lo baja.
        let radius = |t: f64| 1.0 / curve.curvature_at(t).unwrap().abs();
        let at_mark = (0..=2000)
            .map(|k| f64::from(k) / 2000.0)
            .min_by(|a, b| {
                let d = |t: f64| {
                    let p = curve.point_at(t).unwrap();
                    (p.x - z).hypot(p.y - r)
                };
                d(*a).total_cmp(&d(*b))
            })
            .unwrap();
        assert!(
            (radius(at_mark) - shape.min_radius).abs() < 1e-3 * shape.min_radius,
            "{wall:?}: radio en la marca {} frente a {}",
            radius(at_mark),
            shape.min_radius
        );
        let smallest = (0..=2000)
            .map(|k| radius(f64::from(k) / 2000.0))
            .fold(f64::MAX, f64::min);
        assert!(smallest >= shape.min_radius * (1.0 - 1e-3), "{wall:?}");
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
    // Los dos extremos y, tras ellos, P1, P2 y P3 del borde.
    let handles = model.edge_handles();
    assert_eq!(handles.len(), 5);
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

/// Sin editar, el borde es la recta de dos puntos y su Bézier de 5 puntos
/// reparte los puntos a la cuarta parte de la cuerda.
#[test]
fn an_unedited_edge_is_the_straight_line_with_uniform_points() {
    let (_, auto) = model();
    assert_eq!(points(&auto, "Borde de ataque").len(), 2);
    let edge = &auto.params.edge;
    let [from, to] = auto.params.edge_points;
    assert_eq!((edge.points[0], edge.points[4]), (from, to));
    for (k, point) in edge.points.iter().enumerate() {
        let t = k as f64 / 4.0;
        assert!(
            (point[0] - (from[0] + t * (to[0] - from[0]))).abs() < 1e-9,
            "P{k}"
        );
        assert!(
            (point[1] - (from[1] + t * (to[1] - from[1]))).abs() < 1e-9,
            "P{k}"
        );
    }
    assert!((edge.chord - auto.params.edge_length).abs() < 1e-12);
    assert!((edge.auto_arm_length - edge.chord / 4.0).abs() < 1e-12);
    assert_eq!((edge.p2_along, edge.p2_offset), (0.5, 0.0));
    assert_eq!((edge.start_angle, edge.end_angle), (0.0, 0.0));
}

/// Los brazos y P2 se miden en el marco de la cuerda. En un borde radial en
/// z = 0 (de la tapa, arriba, al cubo, abajo) la cuerda baja por −r y la
/// normal, positiva aguas arriba, es −z.
#[test]
fn an_edited_edge_places_p1_p2_and_p3_in_the_chord_frame() {
    let edge = edited(&MeridianEdits {
        edge: EdgeEdit {
            p2_along: Some(0.25),
            p2_offset: Some(3.0),
            start_angle: Some(30.0),
            start_length: Some(5.0),
            end_angle: Some(20.0),
            end_length: Some(6.0),
        },
        ..MeridianEdits::default()
    });
    let shape = &edge.params.edge;
    let [from, to] = edge.params.edge_points;
    assert_eq!(from[0], 0.0);
    assert_eq!(to[0], 0.0);
    let chord = from[1] - to[1];
    let close = |a: [f64; 2], b: [f64; 2], what: &str| {
        assert!(
            (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9,
            "{what}: {a:?} {b:?}"
        );
    };
    let (sin30, cos30) = 30f64.to_radians().sin_cos();
    let (sin20, cos20) = 20f64.to_radians().sin_cos();
    close(shape.points[1], [-5.0 * sin30, from[1] - 5.0 * cos30], "P1");
    close(shape.points[2], [-3.0, from[1] - 0.25 * chord], "P2");
    close(shape.points[3], [-6.0 * sin20, to[1] + 6.0 * cos20], "P3");
    assert_eq!((shape.start_angle, shape.start_length), (30.0, 5.0));
    assert_eq!((shape.end_angle, shape.end_length), (20.0, 6.0));
    // La curva muestreada une los extremos y es más larga que la cuerda.
    let line = points(&edge, "Borde de ataque");
    assert!(line.len() > 2);
    assert_eq!((line[0], line[line.len() - 1]), (from, to));
    let polyline: f64 = line
        .windows(2)
        .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
        .sum();
    assert!((edge.params.edge_length - polyline).abs() < 1e-12);
    assert!(polyline > chord);
    let area: f64 = line
        .windows(2)
        .map(|pair| {
            PI * (pair[0][1] + pair[1][1])
                * (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1])
        })
        .sum();
    assert!((edge.params.edge_area - area).abs() < 1e-9 * area);
    // Los extremos y las tapas no cambian.
    let (_, auto) = model();
    assert_eq!(edge.params.edge_points, auto.params.edge_points);
    for name in ["Tapa delantera", "Tapa trasera"] {
        assert_eq!(points(&edge, name), points(&auto, name), "{name}");
    }
}

#[test]
fn edge_values_out_of_range_are_errors_that_name_them() {
    let bad = |edge: EdgeEdit| {
        meridional_model_with(
            &design(&input()).unwrap(),
            &MeridianEdits {
                edge,
                ..MeridianEdits::default()
            },
        )
        .unwrap_err()
        .to_string()
    };
    for (edge, what) in [
        (
            EdgeEdit {
                start_angle: Some(100.0),
                ..EdgeEdit::default()
            },
            "ángulo de inicio",
        ),
        (
            EdgeEdit {
                end_angle: Some(f64::NAN),
                ..EdgeEdit::default()
            },
            "ángulo final",
        ),
        (
            EdgeEdit {
                start_length: Some(0.0),
                ..EdgeEdit::default()
            },
            "longitud de inicio",
        ),
        (
            EdgeEdit {
                end_length: Some(-1.0),
                ..EdgeEdit::default()
            },
            "longitud final",
        ),
        (
            EdgeEdit {
                p2_along: Some(1.5),
                ..EdgeEdit::default()
            },
            "posición de P2",
        ),
        (
            EdgeEdit {
                p2_offset: Some(f64::INFINITY),
                ..EdgeEdit::default()
            },
            "desvío de P2",
        ),
    ] {
        let error = bad(edge);
        assert!(
            error.contains(what) && error.contains("borde de ataque"),
            "{error}"
        );
    }
}

/// P1, P2 y P3 del borde son movibles: los brazos dan ángulo y longitud con la
/// cuerda y P2, su sitio en el marco; arrastrarlos a donde están devuelve lo
/// editado, y deslizar el puntero de P2 a lo largo de la cuerda no cambia el
/// desvío.
#[test]
fn the_edge_points_are_draggable_in_the_chord_frame() {
    let (_, auto) = model();
    // Recto: cinco puntos movibles, con P1 y P3 unidos a su extremo.
    let handles = auto.edge_handles();
    assert_eq!(handles.len(), 5);
    let edge = &auto.params.edge;
    for handle in handles.iter().filter(|h| h.control.index >= 5) {
        let k = handle.control.index - 4;
        assert_eq!(handle.at, edge.points[k]);
        let expected = match handle.control.index {
            5 => Some((edge.points[0], edge.points[1])),
            7 => Some((edge.points[4], edge.points[3])),
            _ => None,
        };
        assert_eq!(handle.line, expected);
    }
    let control = |index| ControlPoint {
        wall: Wall::Shroud,
        index,
    };
    let edits = EdgeEdit {
        p2_along: Some(0.4),
        p2_offset: Some(2.5),
        start_angle: Some(25.0),
        start_length: Some(7.0),
        end_angle: Some(-15.0),
        end_length: Some(5.5),
    };
    let bent = edited(&MeridianEdits {
        edge: edits,
        ..MeridianEdits::default()
    });
    for (index, expected) in [
        (
            5,
            Moved::Arm {
                angle: 25.0,
                length: 7.0,
            },
        ),
        (
            7,
            Moved::Arm {
                angle: -15.0,
                length: 5.5,
            },
        ),
        (
            6,
            Moved::EdgePoint {
                along: 0.4,
                offset: 2.5,
            },
        ),
    ] {
        let at = bent.control_position(control(index)).unwrap();
        assert_eq!(bent.moved(control(index), at), expected, "P{}", index - 4);
    }
    // Deslizar P2 a lo largo de la cuerda cambia «along», no el desvío.
    let p2 = bent.control_position(control(6)).unwrap();
    let [from, to] = bent.params.edge_points;
    let chord = bent.params.edge.chord;
    let u = [(to[0] - from[0]) / chord, (to[1] - from[1]) / chord];
    let Moved::EdgePoint { along, offset } =
        bent.moved(control(6), [p2[0] + 3.0 * u[0], p2[1] + 3.0 * u[1]])
    else {
        panic!("P2 del borde da EdgePoint");
    };
    assert_eq!(offset, 2.5);
    assert!((along - (0.4 + 3.0 / chord)).abs() < 1e-3);
    // Fuera de la cuerda se recorta a [0, 1].
    let Moved::EdgePoint { along, .. } =
        bent.moved(control(6), [from[0] - 100.0 * u[0], from[1] - 100.0 * u[1]])
    else {
        panic!("P2 del borde da EdgePoint");
    };
    assert_eq!(along, 0.0);
}

/// El canal entero arranca en el plano de entrada, donde empiezan las tapas, y
/// el del borde de ataque es su cola: acaban en la misma salida.
#[test]
fn the_whole_channel_starts_at_the_inlet_plane_and_the_edge_channel_is_its_tail() {
    let (_, auto) = model();
    // Con el borde en el plano de entrada son el mismo canal.
    assert_eq!(auto.channel_from(0.0, 0.0), auto.channel());
    let moved = edited(&MeridianEdits {
        edge_shroud: Some(0.3),
        edge_hub: Some(0.5),
        ..MeridianEdits::default()
    });
    let (whole, tail) = (moved.channel_from(0.0, 0.0), moved.channel());
    assert!(whole.len() > tail.len());
    assert_eq!(tail[0].shroud_point, moved.params.edge_points[0]);
    // El entero parte de P0 de las dos tapas, en z = 0, y sale por d2.
    assert_eq!(whole[0].shroud_point, points(&moved, "Tapa delantera")[0]);
    assert_eq!(whole[0].hub_point, points(&moved, "Tapa trasera")[0]);
    assert_eq!(whole[0].shroud_point[0], 0.0);
    assert_eq!(whole[0].hub_point[0], 0.0);
    let (last_whole, last_tail) = (&whole[whole.len() - 1], &tail[tail.len() - 1]);
    assert_eq!(last_whole.shroud_point, last_tail.shroud_point);
    assert_eq!(last_whole.hub_point, last_tail.hub_point);
}

/// Con el borde curvo, el canal arranca con el área de entrada de la curva, la
/// de la cabecera, y no con la de la cuerda; el resto de estaciones no cambia.
#[test]
fn a_curved_edge_starts_the_channel_with_its_own_inlet_area() {
    let base = MeridianEdits {
        edge_shroud: Some(0.3),
        edge_hub: Some(0.5),
        ..MeridianEdits::default()
    };
    let straight = edited(&base);
    let curved = edited(&MeridianEdits {
        edge: EdgeEdit {
            p2_offset: Some(-15.0),
            ..EdgeEdit::default()
        },
        ..base
    });
    let (before, after) = (straight.channel(), curved.channel());
    // El área de entrada es la de la cabecera en los dos casos.
    assert!((before[0].area - straight.params.edge_area).abs() < 1e-9 * before[0].area);
    assert_eq!(after[0].area, curved.params.edge_area);
    // Curvar el borde cambia el área (más o menos, según hacia dónde se
    // desplace) y la cuerda no la da ya.
    assert!(
        (curved.params.edge_area - straight.params.edge_area).abs()
            > 1e-3 * straight.params.edge_area
    );
    // Misma cuerda: la anchura de la primera estación y todas las demás igual.
    assert_eq!(after[0].width, before[0].width);
    assert_eq!(after[1..], before[1..]);
}

// --- Recorte del cubo -------------------------------------------------------

/// Un diseño con el cubo más largo que el de `input()`: llega a z = 45,6 mm y
/// a z = 37,5 mm su radio pasa de 29 mm, así que cabe el avellanado del plano
/// de B1.
fn big_input() -> DesignInput {
    let q = |text: &str| parse_quantity(text).unwrap();
    DesignInput {
        q: q("175 m3/h"),
        h: q("70 m"),
        ..input()
    }
}

/// Con el borde de ataque al final del cubo, para que el recorte quepa delante.
fn far_edge(cut: HubCutEdit) -> MeridianEdits {
    MeridianEdits {
        edge_hub: Some(0.9),
        hub_cut: cut,
        ..Default::default()
    }
}

fn closed_at(z: f64) -> HubCutEdit {
    HubCutEdit {
        z: Some(z),
        ..Default::default()
    }
}

fn rear_radius_at(model: &MeridionalModel, z: f64) -> f64 {
    let line = points(model, "Tapa trasera");
    line.windows(2)
        .find(|pair| pair[0][0] <= z && z <= pair[1][0])
        .map(|pair| {
            let t = (z - pair[0][0]) / (pair[1][0] - pair[0][0]);
            pair[0][1] + t * (pair[1][1] - pair[0][1])
        })
        .unwrap()
}

fn cut_error(edits: &MeridianEdits) -> String {
    match meridional_model_with(&design(&input()).unwrap(), edits) {
        Err(DesignError::Input(text)) => text,
        other => panic!("se esperaba DesignError::Input, salió {other:?}"),
    }
}

fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

#[test]
fn hub_cut_closed_goes_down_to_the_axis() {
    let model = edited(&far_edge(closed_at(30.0)));
    let cut = points(&model, CUT_NAME);
    assert_eq!(cut.len(), 2);
    assert!(close(cut[0], [30.0, rear_radius_at(&model, 30.0)]));
    assert_eq!(cut[1], [30.0, 0.0]);
    assert_eq!(model.curves.last().unwrap().name, CUT_NAME);
    let shape = model.params.hub_cut.as_ref().unwrap();
    assert_eq!(shape.mode, CutMode::Closed);
    assert_eq!(shape.z, 30.0);
}

#[test]
fn hub_cut_through_with_counterbore_and_chamfer_has_the_expected_contour() {
    let result = design(&big_input()).unwrap();
    let edits = far_edge(HubCutEdit {
        z: Some(37.5),
        mode: CutMode::Through,
        shaft_diameter: Some(41.275),
        counterbore_diameter: Some(58.0),
        counterbore_depth: Some(7.0),
        chamfer: Some(1.0),
    });
    let model = meridional_model_with(&result, &edits).unwrap();
    let rear = points(&model, "Tapa trasera");
    let z_end = rear[rear.len() - 1][0];
    let rc = rear_radius_at(&model, 37.5);
    let cut = points(&model, CUT_NAME);
    let expected = [
        [37.5, rc],
        [37.5, 29.0],
        [44.5, 29.0],
        [44.5, 21.6375],
        [45.5, 20.6375],
        [z_end, 20.6375],
    ];
    assert_eq!(cut.len(), expected.len());
    for (got, want) in cut.iter().zip(expected) {
        assert!(close(*got, want), "{got:?} != {want:?}");
    }
    assert_eq!(model.cut_handle(), Some([37.5, rc]));
}

#[test]
fn hub_cut_through_without_counterbore_goes_straight_to_the_shaft() {
    let edits = far_edge(HubCutEdit {
        z: Some(30.0),
        mode: CutMode::Through,
        shaft_diameter: Some(20.0),
        ..Default::default()
    });
    let model = edited(&edits);
    let cut = points(&model, CUT_NAME);
    let z_end = points(&model, "Tapa trasera").last().unwrap()[0];
    assert_eq!(cut.len(), 3);
    assert_eq!(cut[1], [30.0, 10.0]);
    assert_eq!(cut[2], [z_end, 10.0]);
}

#[test]
fn hub_cut_leaves_the_hydraulic_profile_untouched() {
    let plain = edited(&MeridianEdits {
        edge_hub: Some(0.9),
        ..Default::default()
    });
    let cut = edited(&far_edge(HubCutEdit {
        z: Some(30.0),
        mode: CutMode::Through,
        shaft_diameter: Some(20.0),
        counterbore_diameter: Some(26.0),
        counterbore_depth: Some(3.0),
        chamfer: Some(1.0),
    }));
    for name in [
        "Tapa delantera",
        "Tapa trasera",
        "Borde de ataque",
        "Borde de salida",
    ] {
        assert_eq!(plain.curve(name), cut.curve(name), "{name}");
    }
    assert_eq!(plain.channel(), cut.channel());
    let (a, b) = (&plain.params, &cut.params);
    assert_eq!(a.edge_area, b.edge_area);
    assert_eq!(a.edge_length, b.edge_length);
    assert_eq!(a.edge_points, b.edge_points);
    assert_eq!(a.shroud, b.shroud);
    assert_eq!(a.hub, b.hub);
    assert_eq!(a.d1i, b.d1i);
    assert_eq!(a.z_e, b.z_e);
}

#[test]
fn hub_cut_without_cut_adds_no_curve() {
    let (_, model) = model();
    assert!(model.params.hub_cut.is_none());
    assert_eq!(model.curves.len(), 4);
    assert!(model.curve(CUT_NAME).is_none());
    assert_eq!(model.cut_handle(), None);
}

#[test]
fn hub_cut_at_zero_with_dh_zero_is_valid() {
    let model = edited(&far_edge(closed_at(0.0)));
    let cut = points(&model, CUT_NAME);
    assert!(close(cut[0], [0.0, 0.0]));
    assert_eq!(cut[1], [0.0, 0.0]);
}

#[test]
fn hub_cut_past_the_leading_edge_is_an_error() {
    let edge_z = edited(&MeridianEdits {
        edge_hub: Some(0.5),
        ..Default::default()
    })
    .params
    .edge_points[1][0];
    let text = cut_error(&MeridianEdits {
        edge_hub: Some(0.5),
        hub_cut: closed_at(edge_z + 1.0),
        ..Default::default()
    });
    assert!(text.contains("borde de ataque"), "{text}");
}

#[test]
fn hub_cut_outside_the_hub_is_an_error() {
    let text = cut_error(&far_edge(closed_at(-1.0)));
    assert!(text.contains("-1"), "{text}");
    let text = cut_error(&far_edge(closed_at(1000.0)));
    assert!(text.contains("1000"), "{text}");
}

#[test]
fn hub_cut_shaft_not_smaller_than_hub_is_an_error() {
    let rc = rear_radius_at(&edited(&far_edge(closed_at(30.0))), 30.0);
    let text = cut_error(&far_edge(HubCutEdit {
        z: Some(30.0),
        mode: CutMode::Through,
        shaft_diameter: Some(2.0 * rc + 1.0),
        ..Default::default()
    }));
    assert!(text.contains("eje"), "{text}");
}

#[test]
fn hub_cut_counterbore_not_larger_than_shaft_is_an_error() {
    let text = cut_error(&far_edge(HubCutEdit {
        z: Some(30.0),
        mode: CutMode::Through,
        shaft_diameter: Some(20.0),
        counterbore_diameter: Some(20.0),
        counterbore_depth: Some(3.0),
        ..Default::default()
    }));
    assert!(text.contains("avellanado"), "{text}");
}

#[test]
fn hub_cut_through_without_shaft_is_an_error() {
    let text = cut_error(&far_edge(HubCutEdit {
        z: Some(30.0),
        mode: CutMode::Through,
        ..Default::default()
    }));
    assert!(text.contains("eje"), "{text}");
}

#[test]
fn hub_cut_counterbore_needs_depth_and_chamfer_must_fit() {
    let base = HubCutEdit {
        z: Some(30.0),
        mode: CutMode::Through,
        shaft_diameter: Some(20.0),
        counterbore_diameter: Some(26.0),
        counterbore_depth: Some(3.0),
        chamfer: Some(1.0),
    };
    let text = cut_error(&far_edge(HubCutEdit {
        counterbore_depth: None,
        ..base
    }));
    assert!(text.contains("profundidad"), "{text}");
    let text = cut_error(&far_edge(HubCutEdit {
        chamfer: Some(4.0),
        ..base
    }));
    assert!(text.contains("chaflán"), "{text}");
    let text = cut_error(&far_edge(HubCutEdit {
        chamfer: Some(0.0),
        ..base
    }));
    assert!(text.contains("chaflán"), "{text}");
}

#[test]
fn hub_cut_closed_ignores_counterbore_and_chamfer() {
    let model = edited(&far_edge(HubCutEdit {
        z: Some(30.0),
        mode: CutMode::Closed,
        shaft_diameter: Some(1000.0),
        counterbore_diameter: Some(1.0),
        counterbore_depth: Some(-5.0),
        chamfer: Some(-2.0),
    }));
    assert_eq!(points(&model, CUT_NAME).len(), 2);
}

#[test]
fn hub_cut_follows_the_hub_when_the_edge_changes() {
    let edits = MeridianEdits {
        edge_hub: Some(0.7),
        hub_cut: closed_at(30.0),
        ..Default::default()
    };
    let model = edited(&edits);
    let cut = points(&model, CUT_NAME);
    assert!(close(cut[0], [30.0, rear_radius_at(&model, 30.0)]));
}

#[test]
fn cut_handle_is_the_face_top_point() {
    let model = edited(&far_edge(closed_at(30.0)));
    let rc = rear_radius_at(&model, 30.0);
    assert!(close(model.cut_handle().unwrap(), [30.0, rc]));
}

#[test]
fn the_cut_control_point_resolves_to_the_cut_handle_only_when_there_is_a_cut() {
    use hm_design::meridian::CUT_CONTROL;
    assert_eq!(
        CUT_CONTROL,
        ControlPoint {
            wall: Wall::Hub,
            index: 8
        }
    );
    let model = edited(&far_edge(closed_at(30.0)));
    assert_eq!(model.control_position(CUT_CONTROL), model.cut_handle());
    let (_, plain) = self::model();
    assert_eq!(plain.control_position(CUT_CONTROL), None);
}
