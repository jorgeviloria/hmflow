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
    assert_eq!(
        PieceId::BladeMean { index: 2 }.label(),
        "Superficie media 3"
    );
    assert_eq!(PieceId::Blade { index: 2 }.label(), "Álabe 3");
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

// ---- Álabe con espesor: una pieza cerrada por álabe ----

mod thick {
    use hm_design::blade_camber::{BladeCamber, camber_for_design};
    use hm_design::blade_setup::{BladeSetup, BladeShape};
    use hm_design::blade_solid::{BladeSolid, blade_solid_for_design};
    use hm_design::geometry::{DEFAULT_INCIDENCE_DEG, geometry};
    use hm_design::meridian::{MeridionalModel, meridional_model};
    use hm_design::scene::{BladeFace, BladeSurfaceSource, Piece, PieceId, Scene, scene_from};
    use hm_design::{DesignInput, default_mechanical_efficiency, design};
    use hm_units::parse_quantity;

    use super::distance_to;

    /// El punto de máximo rendimiento de B1, como en `tests/blade_solid.rs`.
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

    struct Thick {
        model: MeridionalModel,
        camber: BladeCamber,
        solid: BladeSolid,
        blades: u32,
    }

    fn thick(shape: BladeShape, streamlines: u8) -> Thick {
        let input = b1_input();
        let result = design(&input).unwrap();
        let impeller = geometry(&input, &result).unwrap().impeller;
        let model = meridional_model(&result).unwrap();
        let setup = BladeSetup {
            shape,
            streamlines,
            ..BladeSetup::default()
        };
        let camber = camber_for_design(&result, &impeller, &model, &setup, DEFAULT_INCIDENCE_DEG)
            .unwrap()
            .expect("forma 3D");
        let solid =
            blade_solid_for_design(&result, &impeller, &model, &setup, DEFAULT_INCIDENCE_DEG)
                .unwrap();
        Thick {
            model,
            camber,
            solid,
            blades: result.selection.blade_count,
        }
    }

    fn scene(case: &Thick, tolerance: f64) -> Scene {
        scene_from(
            &case.model,
            BladeSurfaceSource::Solid {
                camber: &case.camber,
                solid: Ok(&case.solid),
            },
            case.blades,
            tolerance,
        )
    }

    fn blade(scene: &Scene, index: u32) -> &Piece {
        scene
            .piece(PieceId::Blade { index })
            .unwrap_or_else(|| panic!("falta el álabe {index}: {:?}", scene.notices))
    }

    #[test]
    fn a_thick_blade_gives_z_closed_pieces_and_z_mean_surfaces() {
        let case = thick(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        assert!(scene.notices.is_empty(), "{:?}", scene.notices);
        assert_eq!(scene.pieces.len(), 2 + 2 * case.blades as usize);
        for index in 0..case.blades {
            assert!(scene.piece(PieceId::Blade { index }).is_some());
            assert!(scene.piece(PieceId::BladeMean { index }).is_some());
        }
        // Las superficies medias no tienen caras con nombre; las piezas, seis.
        assert!(
            scene
                .piece(PieceId::BladeMean { index: 0 })
                .unwrap()
                .faces
                .is_empty()
        );
        assert_eq!(blade(&scene, 0).faces.len(), 6);
    }

    #[test]
    fn every_blade_mesh_is_closed_and_points_outwards() {
        for shape in [BladeShape::Free3D, BladeShape::Ruled3D] {
            let case = thick(shape, 5);
            for tolerance in [0.05, 0.5] {
                let scene = scene(&case, tolerance);
                for index in [0, case.blades - 1] {
                    let mesh = &blade(&scene, index).surface;
                    assert_eq!(
                        mesh.open_edges(1e-6),
                        0,
                        "{shape:?}, {tolerance} mm, álabe {index}"
                    );
                    assert!(
                        mesh.signed_volume() > 0.0,
                        "{shape:?}, {tolerance} mm: volumen {}",
                        mesh.signed_volume()
                    );
                }
            }
        }
    }

    /// Σ celdas de la superficie media (área × media del espesor real de sus
    /// cuatro esquinas), en mm³.
    fn reference_volume(case: &Thick) -> f64 {
        reference_between(case, &(0..case.camber.lines.len()).collect::<Vec<_>>())
    }

    /// Lo mismo, con solo las líneas dadas (en orden): la superficie entre
    /// dos líneas consecutivas de la lista.
    fn reference_between(case: &Thick, chosen: &[usize]) -> f64 {
        let node = |line: usize, j: usize| {
            let p = &case.camber.lines[line].points[j];
            [
                p.r * p.theta.cos() * 1000.0,
                p.r * p.theta.sin() * 1000.0,
                p.z * 1000.0,
            ]
        };
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let half_area = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
            let n = cross(sub(b, a), sub(c, a));
            0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
        };
        let mut total = 0.0;
        let nodes = case.camber.lines[0].points.len();
        for pair in chosen.windows(2) {
            let (k, next) = (pair[0], pair[1]);
            for j in 0..nodes - 1 {
                let (a, b) = (node(k, j), node(k, j + 1));
                let (c, d) = (node(next, j + 1), node(next, j));
                let area = half_area(a, b, c) + half_area(a, c, d);
                let e = |line: usize, j: usize| case.solid.lines[line].true_thickness[j] * 1000.0;
                let mean = 0.25 * (e(k, j) + e(k, j + 1) + e(next, j) + e(next, j + 1));
                total += area * mean;
            }
        }
        total
    }

    /// El volumen de la malla es el espesor real sobre la superficie media,
    /// dentro del 3 %. La libre pasa por todas las líneas de corriente; la
    /// reglada es la recta de cubo a tapa en cada estación y no pasa por las
    /// intermedias (se apartan hasta 8 mm), así que su referencia usa solo el
    /// cubo y la tapa. Con las cinco líneas, la reglada daría un 4 % de más.
    #[test]
    fn the_blade_volume_is_the_thickness_over_the_mean_surface() {
        for shape in [BladeShape::Free3D, BladeShape::Ruled3D] {
            let case = thick(shape, 5);
            let scene = scene(&case, 0.05);
            let volume = blade(&scene, 0).surface.signed_volume();
            let reference = match shape {
                BladeShape::Ruled3D => reference_between(&case, &[0, case.camber.lines.len() - 1]),
                _ => reference_volume(&case),
            };
            assert!(
                ((volume - reference) / reference).abs() < 0.03,
                "{shape:?}: volumen {volume} mm³ frente a {reference} mm³"
            );
        }
    }

    #[test]
    fn the_hub_band_lies_on_the_hub() {
        let case = thick(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        let band = blade(&scene, 0).face(BladeFace::HubBand).unwrap();
        let wall = &case.model.curve("Tapa trasera").unwrap().points;
        assert!(!band.vertices.is_empty());
        for vertex in &band.vertices {
            let at = [vertex[2], vertex[0].hypot(vertex[1])];
            let off = distance_to(wall, at);
            assert!(off <= 0.05, "a {off} mm del cubo");
        }
    }

    #[test]
    fn the_piece_has_its_twelve_edges() {
        let case = thick(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        let piece = blade(&scene, 0);
        assert_eq!(piece.curves.len(), 12);
        assert!(piece.curves.iter().all(|curve| !curve.closed));
        // Cuatro a lo largo (mismas estaciones), cuatro del canto de entrada y
        // cuatro del de salida: ancho, ancho, 3 y 3.
        let lengths: Vec<usize> = piece.curves.iter().map(|c| c.points.len()).collect();
        assert!(lengths[..4].iter().all(|n| *n == lengths[0] && *n > 2));
        let across = lengths[4];
        assert_eq!(&lengths[4..8], &[across, across, 3, 3]);
        assert_eq!(&lengths[8..12], &[across, across, 3, 3]);
        // Las aristas se encuentran: el final de «presión–cubo» es el comienzo
        // del canto de salida de presión (mismos puntos, bit a bit).
        assert_eq!(
            piece.curves[0].points.last(),
            piece.curves[8].points.first()
        );
        assert_eq!(
            piece.curves[0].points.first(),
            piece.curves[4].points.first()
        );
    }

    #[test]
    fn the_piece_has_six_named_faces_that_cover_it() {
        let case = thick(BladeShape::Ruled3D, 5);
        let scene = scene(&case, 0.05);
        let piece = blade(&scene, 0);
        let names: Vec<BladeFace> = piece.faces.iter().map(|(face, _)| *face).collect();
        assert_eq!(names, BladeFace::ALL);
        let mut next = 0;
        for (face, range) in &piece.faces {
            assert_eq!(
                range.start, next,
                "{face:?} no empieza donde acabó la anterior"
            );
            assert!(range.end > range.start, "{face:?} está vacía");
            next = range.end;
        }
        assert_eq!(next as usize, piece.surface.triangles.len());
        // Cada cara se lee como malla propia con sus triángulos.
        for (face, range) in &piece.faces {
            let mesh = piece.face(*face).unwrap();
            assert_eq!(mesh.triangles.len(), (range.end - range.start) as usize);
            assert_eq!(mesh.normals.len(), mesh.vertices.len());
        }
        assert_eq!(BladeFace::Pressure.label(), "Cara de presión");
        assert_eq!(BladeFace::TrailingEdge.label(), "Canto de salida");
    }

    #[test]
    fn the_edge_faces_are_apart_from_the_side_faces() {
        let case = thick(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        let piece = blade(&scene, 0);
        let pressure = piece.face(BladeFace::Pressure).unwrap();
        let suction = piece.face(BladeFace::Suction).unwrap();
        let leading = piece.face(BladeFace::LeadingEdge).unwrap();
        let trailing = piece.face(BladeFace::TrailingEdge).unwrap();
        let across = leading.vertices.len() / 3;
        assert_eq!(trailing.vertices.len(), 3 * across);
        // Presión y succión son estaciones × columnas: la primera estación son
        // sus primeras `across` filas de vértices y la última, las últimas.
        let first = |mesh: &hm_geometry::mesh::TriangleMesh| mesh.vertices[..across].to_vec();
        let last = |mesh: &hm_geometry::mesh::TriangleMesh| {
            mesh.vertices[mesh.vertices.len() - across..].to_vec()
        };
        // Filas del canto: presión, medio y succión.
        assert_eq!(leading.vertices[..across], first(&pressure));
        assert_eq!(leading.vertices[2 * across..], first(&suction));
        assert_eq!(trailing.vertices[..across], last(&pressure));
        assert_eq!(trailing.vertices[2 * across..], last(&suction));
        // Ningún triángulo del canto toca las caras laterales: son mallas
        // propias, con 2·2·(columnas − 1) triángulos.
        assert_eq!(leading.triangles.len(), 4 * (across - 1));
        assert_eq!(trailing.triangles.len(), 4 * (across - 1));
    }

    #[test]
    fn blade_k_is_blade_0_turned() {
        let case = thick(BladeShape::Free3D, 5);
        let scene = scene(&case, 0.05);
        let first = blade(&scene, 0);
        for k in 1..case.blades {
            let angle = -std::f64::consts::TAU * f64::from(k) / f64::from(case.blades);
            let turned = first.surface.rotated_z(angle);
            let piece = blade(&scene, k);
            assert_eq!(piece.surface.triangles, turned.triangles);
            assert_eq!(piece.faces, first.faces);
            for (a, b) in piece.surface.vertices.iter().zip(&turned.vertices) {
                for axis in 0..3 {
                    assert!((a[axis] - b[axis]).abs() <= 1e-9, "álabe {k}");
                }
            }
        }
    }

    #[test]
    fn a_failed_solid_falls_back_to_the_mean_surface_with_its_reason() {
        let case = thick(BladeShape::Free3D, 5);
        let scene = scene_from(
            &case.model,
            BladeSurfaceSource::Solid {
                camber: &case.camber,
                solid: Err("los álabes vecinos se tocan".to_string()),
            },
            case.blades,
            0.05,
        );
        assert!(
            (0..case.blades).all(|index| scene.piece(PieceId::Blade { index }).is_none()),
            "salió algún álabe con espesor"
        );
        assert!((0..case.blades).all(|index| scene.piece(PieceId::BladeMean { index }).is_some()));
        let notice = scene
            .notices
            .iter()
            .find(|notice| notice.piece == PieceId::Blade { index: 0 })
            .expect("falta el aviso");
        assert_eq!(
            notice.message,
            "el álabe con espesor no sale (los álabes vecinos se tocan); se dibuja la superficie media"
        );
    }

    #[test]
    fn labels_tell_blade_and_mean_surface_apart() {
        assert_ne!(
            PieceId::Blade { index: 0 }.label(),
            PieceId::BladeMean { index: 0 }.label()
        );
        assert_eq!(PieceId::Blade { index: 0 }.label(), "Álabe 1");
        assert_eq!(
            PieceId::BladeMean { index: 0 }.label(),
            "Superficie media 1"
        );
    }
}
