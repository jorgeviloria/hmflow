//! Escena 3D: cubo y tapa son superficies de revolución del meridiano.

use hm_design::meridian::{MeridionalModel, Wall, meridional_model};
use hm_design::scene::{
    BladeSurfaceSource, PieceId, SCENE_TOLERANCE_FINE_MM, SCENE_TOLERANCE_RANGE_MM, Scene,
    scene_from,
};
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

fn model() -> MeridionalModel {
    let result = design(&input()).unwrap();
    meridional_model(&result).unwrap()
}

fn scene(tolerance: f64) -> Scene {
    scene_from(
        &model(),
        BladeSurfaceSource::Unavailable("x".into()),
        6,
        tolerance,
    )
}

/// Distancia de `point` a la polilínea `line`, en el plano `[z, r]`.
fn distance_to(line: &[[f64; 2]], point: [f64; 2]) -> f64 {
    line.windows(2)
        .map(|pair| {
            let [a, b] = [pair[0], pair[1]];
            let d = [b[0] - a[0], b[1] - a[1]];
            let length2 = d[0] * d[0] + d[1] * d[1];
            let t = if length2 > 0.0 {
                (((point[0] - a[0]) * d[0] + (point[1] - a[1]) * d[1]) / length2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (point[0] - a[0] - t * d[0]).hypot(point[1] - a[1] - t * d[1])
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn hub_and_shroud_are_surfaces_of_revolution_of_the_meridian() {
    let model = model();
    let scene = scene_from(&model, BladeSurfaceSource::Unavailable("x".into()), 6, 0.05);
    for (id, wall) in [(PieceId::Hub, Wall::Hub), (PieceId::Shroud, Wall::Shroud)] {
        let piece = scene.piece(id).unwrap_or_else(|| panic!("falta {id:?}"));
        let line = &model.curve(wall.name()).unwrap().points;
        assert!(!piece.surface.triangles.is_empty(), "{id:?} sin triángulos");
        for vertex in &piece.surface.vertices {
            let at = [vertex[2], vertex[0].hypot(vertex[1])];
            let gap = distance_to(line, at);
            assert!(gap <= 1e-6, "{id:?}: {vertex:?} a {gap} mm del meridiano");
        }
        assert_eq!(piece.curves.len(), 2, "{id:?}");
        for curve in &piece.curves {
            assert!(curve.closed, "{id:?}");
            for point in &curve.points {
                let at = [point[2], point[0].hypot(point[1])];
                assert!(distance_to(line, at) <= 1e-6, "{id:?}: {point:?}");
            }
        }
    }
    assert_eq!(scene.notices.len(), 1);
    assert_eq!(scene.notices[0].piece, PieceId::BladeMean { index: 0 });
    assert_eq!(scene.notices[0].message, "x");
    assert!(scene.piece(PieceId::BladeMean { index: 0 }).is_none());
}

#[test]
fn the_tolerance_is_clamped_and_reported() {
    let (low, high) = SCENE_TOLERANCE_RANGE_MM;
    assert_eq!(scene(0.0).tolerance, low);
    assert_eq!(scene(-1.0).tolerance, low);
    assert_eq!(scene(f64::NAN).tolerance, SCENE_TOLERANCE_FINE_MM);
    assert_eq!(scene(1e6).tolerance, high);
    assert_eq!(scene(f64::INFINITY).tolerance, high);
    let finest = scene(1e-9);
    assert_eq!(finest.tolerance, low);
    assert!(
        finest.triangle_count() < 5_000_000,
        "{}",
        finest.triangle_count()
    );
    assert_eq!(scene(0.05).tolerance, 0.05);
}

#[test]
fn a_coarser_tolerance_gives_fewer_triangles() {
    assert!(scene(0.5).triangle_count() < scene(0.05).triangle_count());
}

#[test]
fn the_fingerprint_changes_with_the_geometry_and_not_between_runs() {
    assert_eq!(scene(0.05).fingerprint(), scene(0.05).fingerprint());
    assert_ne!(scene(0.05).fingerprint(), scene(0.5).fingerprint());
}

#[test]
fn the_bounds_enclose_hub_and_shroud() {
    let scene = scene(0.05);
    let (low, high) = scene.bounds().unwrap();
    for piece in &scene.pieces {
        let (piece_low, piece_high) = piece.surface.bounds().unwrap();
        for axis in 0..3 {
            assert!(low[axis] <= piece_low[axis] && piece_high[axis] <= high[axis]);
        }
    }
    let empty = Scene {
        pieces: Vec::new(),
        notices: Vec::new(),
        tolerance: SCENE_TOLERANCE_FINE_MM,
    };
    assert_eq!(empty.bounds(), None);
    assert_eq!(empty.triangle_count(), 0);
}

#[test]
fn a_missing_wall_is_a_notice_and_not_a_panic() {
    let mut model = model();
    model.curves.retain(|curve| curve.name != Wall::Hub.name());
    let scene = scene_from(&model, BladeSurfaceSource::Unavailable("x".into()), 6, 0.05);
    assert!(scene.piece(PieceId::Hub).is_none());
    assert!(scene.piece(PieceId::Shroud).is_some());
    let notice = scene
        .notices
        .iter()
        .find(|notice| notice.piece == PieceId::Hub)
        .expect("aviso en el cubo");
    assert!(
        notice.message.contains("Tapa trasera"),
        "{}",
        notice.message
    );
}

#[test]
fn a_wall_that_cannot_revolve_is_a_notice() {
    let mut model = model();
    for curve in &mut model.curves {
        if curve.name == Wall::Shroud.name() {
            curve.points = vec![[0.0, 10.0]];
        }
    }
    let scene = scene_from(&model, BladeSurfaceSource::Unavailable("x".into()), 6, 0.05);
    assert!(scene.piece(PieceId::Shroud).is_none());
    assert!(scene.piece(PieceId::Hub).is_some());
    assert!(
        scene
            .notices
            .iter()
            .any(|notice| notice.piece == PieceId::Shroud && notice.message.contains("tapa"))
    );
}

#[test]
fn labels_are_in_spanish() {
    assert_eq!(PieceId::Hub.label(), "Cubo");
    assert_eq!(PieceId::Shroud.label(), "Tapa");
    assert_eq!(PieceId::BladeMean { index: 2 }.label(), "Álabe 3");
}

// ---- Superficie media de los álabes (formas 3D) ----

mod blades {
    use hm_design::blade_camber::{BladeCamber, camber_for_design};
    use hm_design::blade_setup::{BladeSetup, BladeShape};
    use hm_design::geometry::{DEFAULT_INCIDENCE_DEG, geometry};
    use hm_design::meridian::{MeridionalModel, Wall, meridional_model};
    use hm_design::plan::plan_model_with;
    use hm_design::scene::{BladeSurfaceSource, PieceId, Scene, scene_from};
    use hm_design::{DesignInput, DesignResult, default_mechanical_efficiency, design};
    use hm_units::parse_quantity;

    use super::distance_to;

    /// El punto de máximo rendimiento de B1, como en `tests/plan.rs`.
    fn b1_input() -> DesignInput {
        let q = parse_quantity("1060 gpm").unwrap();
        let n = parse_quantity("1750 rpm").unwrap();
        DesignInput {
            h: parse_quantity("83 m").unwrap(),
            rho: parse_quantity("998.2 kg/m3").unwrap(),
            eta_m: default_mechanical_efficiency(&q, &n).unwrap(),
            npsha: None,
            nu: None,
            q,
            n,
        }
    }

    struct Case {
        result: DesignResult,
        model: MeridionalModel,
        camber: BladeCamber,
        blades: u32,
    }

    fn case(shape: BladeShape, streamlines: u8) -> Case {
        let input = b1_input();
        let result = design(&input).unwrap();
        let shape_geometry = geometry(&input, &result).unwrap();
        let model = meridional_model(&result).unwrap();
        let setup = BladeSetup {
            shape,
            streamlines,
            ..BladeSetup::default()
        };
        let camber = camber_for_design(
            &result,
            &shape_geometry.impeller,
            &model,
            &setup,
            DEFAULT_INCIDENCE_DEG,
        )
        .unwrap()
        .expect("forma 3D");
        let blades = result.selection.blade_count;
        Case {
            result,
            model,
            camber,
            blades,
        }
    }

    fn scene(case: &Case, tolerance: f64) -> Scene {
        scene_from(
            &case.model,
            BladeSurfaceSource::Camber(&case.camber),
            case.blades,
            tolerance,
        )
    }

    fn blade(scene: &Scene, index: u32) -> &hm_design::scene::Piece {
        scene
            .piece(PieceId::BladeMean { index })
            .unwrap_or_else(|| panic!("falta el álabe {index}: {:?}", scene.notices))
    }

    #[test]
    fn a_free_blade_gives_z_mean_surfaces() {
        let case = case(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        assert!(scene.notices.is_empty(), "{:?}", scene.notices);
        assert!(scene.piece(PieceId::Hub).is_some());
        assert!(scene.piece(PieceId::Shroud).is_some());
        assert_eq!(scene.pieces.len(), 2 + case.blades as usize);
        for index in 0..case.blades {
            let piece = blade(&scene, index);
            assert!(!piece.surface.triangles.is_empty());
            assert_eq!(piece.curves.len(), 4);
        }
    }

    /// Una superficie tan doblada a lo ancho que ni con 64 columnas cumple
    /// la tolerancia sale igual, pero con un aviso: no se calla.
    #[test]
    fn a_blade_that_misses_the_tolerance_says_so() {
        let mut warped = case(BladeShape::Free3D, 3);
        for point in &mut warped.camber.lines[1].points {
            point.theta += 1.0;
        }
        let scene = scene(&warped, 0.005);
        assert!(scene.piece(PieceId::BladeMean { index: 0 }).is_some());
        assert!(
            scene.notices.iter().any(|notice| notice
                .message
                .contains("no alcanza la tolerancia de 0.005 mm")),
            "{:?}",
            scene.notices
        );
        let fine = scene_from(
            &warped.model,
            BladeSurfaceSource::Camber(&case(BladeShape::Free3D, 3).camber),
            warped.blades,
            0.05,
        );
        assert!(fine.notices.is_empty(), "{:?}", fine.notices);
    }

    #[test]
    fn the_blade_edges_lie_on_hub_and_shroud() {
        let case = case(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        let piece = blade(&scene, 0);
        for (curve, wall) in [
            (&piece.curves[2], Wall::Hub),
            (&piece.curves[3], Wall::Shroud),
        ] {
            let line = &case.model.curve(wall.name()).unwrap().points;
            for point in &curve.points {
                let gap = distance_to(line, [point[2], point[0].hypot(point[1])]);
                assert!(gap <= 0.01, "{} a {gap} mm", wall.name());
            }
        }
    }

    #[test]
    fn the_hub_edge_seen_from_suction_is_the_plan_curve() {
        let case = case(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        let geometry = geometry(&b1_input(), &case.result).unwrap();
        let plan = plan_model_with(&case.result, &geometry, Some(&case.camber)).unwrap();
        for k in 0..2u32 {
            let plan_hub = &plan
                .curve(&format!("Álabe {} · cubo", k + 1))
                .unwrap()
                .points;
            let hub = &blade(&scene, k).curves[2].points;
            for point in hub {
                let seen = [-point[0], point[1]];
                let gap = plan_hub
                    .iter()
                    .map(|p| (p[0] - seen[0]).hypot(p[1] - seen[1]))
                    .fold(f64::INFINITY, f64::min);
                assert!(gap <= 1e-6, "álabe {k}: {gap} mm");
            }
            // Los extremos son los de la planta.
            for (scene_end, plan_end) in [
                (hub.first().unwrap(), plan_hub.first().unwrap()),
                (hub.last().unwrap(), plan_hub.last().unwrap()),
            ] {
                assert!((-scene_end[0] - plan_end[0]).hypot(scene_end[1] - plan_end[1]) <= 1e-6);
            }
        }
    }

    #[test]
    fn the_blade_goes_clockwise_seen_from_suction() {
        let case = case(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        let hub = &blade(&scene, 0).curves[2].points;
        let mut previous = hub[0][1].atan2(-hub[0][0]);
        let mut total = 0.0;
        for point in &hub[1..] {
            let angle = point[1].atan2(-point[0]);
            let mut step = angle - previous;
            while step > std::f64::consts::PI {
                step -= std::f64::consts::TAU;
            }
            while step < -std::f64::consts::PI {
                step += std::f64::consts::TAU;
            }
            assert!(step < 0.0, "paso {step}");
            total += step;
            previous = angle;
        }
        let wrap = case.camber.lines[0].wrap;
        assert!((total + wrap).abs() <= 1e-6, "total {total}, wrap {wrap}");
    }

    #[test]
    fn a_ruled_blade_is_straight_from_hub_to_shroud() {
        let case = case(BladeShape::Ruled3D, 5);
        let scene = scene(&case, 0.05);
        assert!(scene.notices.is_empty(), "{:?}", scene.notices);
        let piece = blade(&scene, 0);
        let hub = &piece.curves[2].points;
        let shroud = &piece.curves[3].points;
        let mesh = &piece.surface;
        let cols = mesh.vertices.len() / hub.len();
        assert_eq!(cols * hub.len(), mesh.vertices.len());
        assert!(cols >= 8);
        for (row, (a, b)) in hub.iter().zip(shroud).enumerate() {
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let length2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            for vertex in &mesh.vertices[row * cols..(row + 1) * cols] {
                let v = [vertex[0] - a[0], vertex[1] - a[1], vertex[2] - a[2]];
                let t = ((v[0] * d[0] + v[1] * d[1] + v[2] * d[2]) / length2).clamp(0.0, 1.0);
                let gap = (0..3)
                    .map(|axis| (v[axis] - t * d[axis]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                assert!(gap <= 1e-9, "fila {row}: {gap} mm");
            }
        }
    }

    #[test]
    fn blade_k_is_blade_0_turned() {
        let case = case(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        let first = &blade(&scene, 0).surface;
        for k in 1..case.blades {
            let angle = -std::f64::consts::TAU * f64::from(k) / f64::from(case.blades);
            let turned = first.rotated_z(angle);
            let piece = &blade(&scene, k).surface;
            assert_eq!(piece.triangles, turned.triangles);
            for (a, b) in piece.vertices.iter().zip(&turned.vertices) {
                for axis in 0..3 {
                    assert!((a[axis] - b[axis]).abs() <= 1e-9, "álabe {k}");
                }
            }
        }
    }

    #[test]
    fn a_coarser_tolerance_keeps_the_edges() {
        let case = case(BladeShape::Free3D, 5);
        let fine = scene(&case, 0.05);
        let coarse = scene(&case, 0.5);
        let (fine, coarse) = (blade(&fine, 0), blade(&coarse, 0));
        assert!(coarse.surface.triangles.len() < fine.surface.triangles.len());
        for (a, b) in fine.curves.iter().zip(&coarse.curves) {
            assert_eq!(a.points.first(), b.points.first());
            assert_eq!(a.points.last(), b.points.last());
        }
    }

    // ---- Forma 2D: la línea media de siempre, tendida entre cubo y tapa ----

    struct Planar {
        result: DesignResult,
        model: MeridionalModel,
        blade: Vec<hm_design::geometry::BladePoint>,
        blades: u32,
    }

    fn planar_case() -> Planar {
        let input = b1_input();
        let result = design(&input).unwrap();
        let shape = geometry(&input, &result).unwrap();
        let model = meridional_model(&result).unwrap();
        let blades = result.selection.blade_count;
        Planar {
            blade: shape.impeller.blade.clone(),
            result,
            model,
            blades,
        }
    }

    fn planar_scene(case: &Planar) -> Scene {
        scene_from(
            &case.model,
            BladeSurfaceSource::Planar(&case.blade),
            case.blades,
            0.05,
        )
    }

    #[test]
    fn a_planar_blade_is_cylindrical() {
        let case = planar_case();
        let scene = planar_scene(&case);
        assert!(scene.notices.is_empty(), "{:?}", scene.notices);
        assert_eq!(scene.pieces.len(), 2 + case.blades as usize);
        let piece = blade(&scene, 0);
        assert_eq!(piece.curves.len(), 4);
        for (hub, shroud) in piece.curves[2].points.iter().zip(&piece.curves[3].points) {
            assert!((hub[0] - shroud[0]).abs() <= 1e-9 && (hub[1] - shroud[1]).abs() <= 1e-9);
        }
    }

    #[test]
    fn the_planar_blade_seen_from_suction_is_the_plan_curve() {
        let case = planar_case();
        let scene = planar_scene(&case);
        let shape = geometry(&b1_input(), &case.result).unwrap();
        let plan = hm_design::plan::plan_model(&case.result, &shape).unwrap();
        let plan_blade = &plan.curve("Álabe 1").unwrap().points;
        let hub = &blade(&scene, 0).curves[2].points;
        assert_eq!(hub.len(), plan_blade.len());
        for (point, expected) in hub.iter().zip(plan_blade) {
            let gap = (-point[0] - expected[0]).hypot(point[1] - expected[1]);
            assert!(gap <= 1e-6, "{gap} mm");
        }
    }

    #[test]
    fn the_planar_blade_edges_lie_on_the_walls() {
        let case = planar_case();
        let scene = planar_scene(&case);
        let piece = blade(&scene, 0);
        for (curve, wall) in [
            (&piece.curves[2], Wall::Hub),
            (&piece.curves[3], Wall::Shroud),
        ] {
            let line = &case.model.curve(wall.name()).unwrap().points;
            for point in &curve.points {
                let gap = distance_to(line, [point[2], point[0].hypot(point[1])]);
                assert!(gap <= 0.01, "{} a {gap} mm", wall.name());
            }
        }
    }

    #[test]
    fn a_wall_without_a_single_z_leaves_no_blades_and_says_why() {
        let mut case = planar_case();
        // Una tapa sintética que se dobla sobre sí misma en todo el álabe.
        let shroud = case
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.name == Wall::Shroud.name())
            .unwrap();
        shroud.points = vec![[0.0, 1.0], [100.0, 400.0], [200.0, 1.0], [300.0, 400.0]];
        let scene = planar_scene(&case);
        assert!(scene.piece(PieceId::Hub).is_some());
        assert!(scene.piece(PieceId::Shroud).is_some());
        assert!(
            !scene
                .pieces
                .iter()
                .any(|piece| matches!(piece.id, PieceId::BladeMean { .. }))
        );
        assert_eq!(scene.notices.len(), 1, "{:?}", scene.notices);
        let notice = &scene.notices[0];
        assert_eq!(notice.piece, PieceId::BladeMean { index: 0 });
        let r1 = case.blade[0].r * 1000.0;
        assert_eq!(
            notice.message,
            format!(
                "la forma 2D no se puede tender entre cubo y tapa: la tapa no tiene un único z en r = {r1:.1} mm"
            )
        );
    }

    #[test]
    fn two_streamlines_are_enough() {
        let mut case = case(BladeShape::Free3D, 3);
        // Solo cubo y tapa: la spline se queda en recta.
        let last = case.camber.lines.pop().unwrap();
        case.camber.lines.truncate(1);
        case.camber.lines.push(last);
        let scene = scene(&case, 0.05);
        assert!(scene.notices.is_empty(), "{:?}", scene.notices);
        assert!(!blade(&scene, 0).surface.triangles.is_empty());
    }
}
