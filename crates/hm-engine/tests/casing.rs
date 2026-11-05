//! `[casing]` a través del motor: lo que el proyecto declara llega al diseño
//! y el ψ guardado no bloquea cambiar el punto.

use hm_engine::{CurvePoints, analyze, audit, case_from_project, recalc};
use hm_project::{Casing, Param, Project, PumpType};

fn casing() -> Casing {
    Casing {
        d2: "210 mm".to_string(),
        neck_diameter: "90 mm".to_string(),
        eye_diameter_max: "75 mm".to_string(),
        bore_diameter: "20 mm".to_string(),
        hub_diameter: "30 mm".to_string(),
        b3: "30 mm".to_string(),
        cutwater_radius: "111 mm".to_string(),
        channels: 1,
        wrap: "180 deg".to_string(),
        throat_area: String::new(),
    }
}

fn project() -> Project {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.casing = Some(casing());
    project
}

fn saved() -> Project {
    recalc(project(), false).unwrap().project
}

fn psi_of(project: &Project) -> f64 {
    match project.params.get("psi") {
        Some(Param::Manual { value } | Param::Locked { value }) => value.parse().unwrap(),
        other => panic!("psi inesperado: {other:?}"),
    }
}

#[test]
fn the_casing_reaches_the_design_in_si() {
    let case = case_from_project(&project()).unwrap();
    let fit = case.design.casing.as_ref().unwrap();
    assert_eq!(fit.d2, 0.21);
    assert_eq!(fit.neck_diameter, 0.09);
    assert_eq!(fit.channels, 1);
    assert!((fit.wrap_deg - 180.0).abs() < 1e-9);
    assert_eq!(fit.throat_area, None);
    let analysis = analyze(&case, CurvePoints::MINIMUM).unwrap();
    assert!((analysis.result.d2.si - 0.21).abs() < 1e-12);
}

#[test]
fn a_bad_casing_names_its_field() {
    let mut project = project();
    project.casing.as_mut().unwrap().d2 = "mucho".to_string();
    let error = case_from_project(&project).unwrap_err();
    assert_eq!(error.code(), Some("HM-PROJ-030"));
    assert!(error.to_string().contains("`d2`"), "{error}");
}

#[test]
fn changing_the_head_updates_the_saved_psi() {
    let mut project = saved();
    let before = psi_of(&project);
    project.operating.h = "46 m".to_string();
    let project = recalc(project, false).unwrap().project;
    assert!((psi_of(&project) / before - 46.0 / 45.0).abs() < 1e-9);
}

#[test]
fn recalculating_twice_changes_nothing() {
    let once = saved();
    let twice = recalc(once.clone(), false).unwrap().project;
    assert_eq!(psi_of(&once), psi_of(&twice));
}

#[test]
fn reselecting_keeps_the_casing() {
    let project = recalc(saved(), true).unwrap().project;
    let case = case_from_project(&project).unwrap();
    let analysis = analyze(&case, CurvePoints::MINIMUM).unwrap();
    assert!((analysis.result.d2.si - 0.21).abs() < 1e-12);
    assert!(project.casing.is_some());
}

#[test]
fn a_locked_psi_that_disagrees_is_still_an_error() {
    let mut project = saved();
    project.params.insert(
        "psi".to_string(),
        Param::Locked {
            value: "1.1".to_string(),
        },
    );
    let error = recalc(project, false).unwrap_err().to_string();
    assert!(error.contains("[casing]"), "{error}");
}

#[test]
fn the_casing_psi_is_not_reported_as_stale() {
    let diagnostics = audit(&saved());
    let found: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("psi"))
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    assert!(found.is_empty(), "{found:?}");
}
