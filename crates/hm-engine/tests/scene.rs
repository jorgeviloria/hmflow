//! La escena 3D de un proyecto: la misma cadena que la planta.

use hm_design::scene::PieceId;
use hm_engine::{
    Case, CurvePoints, Density, DensitySource, Duty, EngineError, Entered, Fluid, FluidKind,
    Intake, PumpType, SCENE_TOLERANCE_DRAG_MM, SCENE_TOLERANCE_FINE_MM, analyze, new_project,
    parse_temperature,
};
use hm_project::{Param, Project};
use hm_units::parse_quantity;

fn entered(text: &str) -> Entered {
    Entered::parse(text).unwrap()
}

fn b1() -> Case {
    Case {
        pump_type: PumpType::EndSuction,
        duty: Duty {
            q: entered("1060 gpm"),
            h: entered("83 m"),
            n: entered("1750 rpm"),
        },
        fluid: Fluid {
            kind: FluidKind::Water,
            temperature: Some(parse_temperature("20 degC").unwrap()),
            density: Density {
                value: parse_quantity("998.2 kg/m3").unwrap(),
                source: DensitySource::Explicit {
                    text: "998.2 kg/m3".to_string(),
                },
            },
            viscosity: None,
        },
        intake: Intake::None,
        mechanical_efficiency: None,
        design: hm_design::DesignOptions::default(),
    }
}

fn b1_project() -> Project {
    new_project(
        "b1",
        &b1(),
        &analyze(&b1(), CurvePoints::new(11).unwrap()).unwrap(),
    )
    .unwrap()
}

#[test]
fn the_scene_of_b1_has_hub_and_shroud() {
    let scene = hm_engine::scene(&b1_project(), SCENE_TOLERANCE_FINE_MM).unwrap();
    assert!(scene.piece(PieceId::Hub).is_some());
    assert!(scene.piece(PieceId::Shroud).is_some());
    assert_eq!(scene.tolerance, SCENE_TOLERANCE_FINE_MM);
    // Sin recorte que falle, el único aviso es el del álabe.
    assert_eq!(scene.notices.len(), 1, "{:?}", scene.notices.len());
    assert_eq!(scene.notices[0].piece, PieceId::BladeMean { index: 0 });
}

#[test]
fn the_drag_tolerance_gives_fewer_triangles() {
    let project = b1_project();
    let fine = hm_engine::scene(&project, SCENE_TOLERANCE_FINE_MM).unwrap();
    let drag = hm_engine::scene(&project, SCENE_TOLERANCE_DRAG_MM).unwrap();
    assert!(drag.triangle_count() < fine.triangle_count());
}

#[test]
fn an_invalid_project_has_no_scene() {
    let mut project = b1_project();
    project.params.insert(
        "psi".to_string(),
        Param::Override {
            expr: "0.5".to_string(),
            author: "test".to_string(),
            date: "2026-11-10".to_string(),
            reason: "prueba".to_string(),
        },
    );
    let error = hm_engine::scene(&project, SCENE_TOLERANCE_FINE_MM).unwrap_err();
    assert_eq!(error.code(), Some("HM-PROJ-019"));

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hm-project/tests/fixtures/end_suction_v0.8.hmf");
    let legacy = Project::load(&path).unwrap();
    let error = hm_engine::scene(&legacy, SCENE_TOLERANCE_FINE_MM).unwrap_err();
    assert!(
        matches!(error, EngineError::LegacyMethod { .. }),
        "{error:?}"
    );
}

/// Un recorte de cubo que falla deja el cubo sin recorte y lo dice.
#[test]
fn a_failing_hub_cut_draws_the_uncut_hub_with_a_notice() {
    let plain = hm_engine::scene(&b1_project(), SCENE_TOLERANCE_FINE_MM).unwrap();
    let mut project = b1_project();
    project.meridian = Some(hm_project::Meridian {
        hub_cut: hm_project::MeridianHubCut {
            // El cubo acaba mucho antes de 500 mm.
            z: "500 mm".into(),
            ..hm_project::MeridianHubCut::default()
        },
        ..hm_project::Meridian::default()
    });
    assert!(
        hm_engine::meridian(&project).is_err(),
        "el recorte tiene que fallar para que la prueba valga"
    );
    let scene = hm_engine::scene(&project, SCENE_TOLERANCE_FINE_MM).unwrap();
    assert_eq!(
        scene.piece(PieceId::Hub).unwrap().surface,
        plain.piece(PieceId::Hub).unwrap().surface
    );
    let notice = scene
        .notices
        .iter()
        .find(|notice| notice.piece == PieceId::Hub)
        .expect("aviso en el cubo");
    assert!(
        notice
            .message
            .starts_with("el recorte del cubo no se pudo aplicar ("),
        "{}",
        notice.message
    );
    assert!(
        notice.message.ends_with("); se dibuja el cubo sin recorte"),
        "{}",
        notice.message
    );
}

/// Con forma 3D, libre o reglada, la escena lleva los Z álabes y ningún aviso.
#[test]
fn a_3d_blade_gives_z_blades_in_the_scene() {
    for shape in ["free_3d", "ruled_3d"] {
        let mut project = b1_project();
        project.blade = Some(hm_project::Blade {
            shape: shape.into(),
            streamlines: 5,
            ..hm_project::Blade::default()
        });
        let scene = hm_engine::scene(&project, SCENE_TOLERANCE_FINE_MM).unwrap();
        assert!(scene.notices.is_empty(), "{shape}: {:?}", scene.notices);
        let blades = scene
            .pieces
            .iter()
            .filter(|piece| matches!(piece.id, PieceId::BladeMean { .. }))
            .count();
        assert_eq!(blades, 5, "{shape}");
    }
}
