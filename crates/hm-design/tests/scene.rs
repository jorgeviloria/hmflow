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
