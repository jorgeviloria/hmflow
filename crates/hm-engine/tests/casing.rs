//! `[casing]` a través del motor: lo que el proyecto declara llega al diseño
//! y el ψ guardado no bloquea cambiar el punto.

use hm_engine::{
    CurvePoints, analyze, audit, case_from_project, new_casing_project, new_project, recalc, report,
};
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
        wrap: String::new(),
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
    assert_eq!(fit.wrap_deg, None);
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
fn recalculating_twice_changes_nothing_and_reports_nothing() {
    let once = saved();
    let twice = recalc(once.clone(), false).unwrap();
    assert_eq!(once.to_toml().unwrap(), twice.project.to_toml().unwrap());
    assert!(twice.changes.is_empty(), "{:?}", twice.changes);
}

#[test]
fn a_real_change_of_the_eye_is_reported_once() {
    let mut project = saved();
    project.operating.q = "60 m3/h".to_string();
    let changes = recalc(project, false).unwrap().changes;
    let names: Vec<_> = changes.iter().map(|change| change.name.as_str()).collect();
    assert!(names.contains(&"c0"), "{names:?}");
    assert_eq!(names.iter().filter(|name| **name == "c0").count(), 1);
    let c0 = changes.iter().find(|change| change.name == "c0").unwrap();
    assert!(c0.before.is_some());
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
fn casing_results_are_never_reported_as_stale() {
    let mut project = saved();
    project.operating.h = "46 m".to_string();
    project.operating.q = "130 m3/h".to_string();
    let stale: Vec<_> = audit(&project)
        .iter()
        .filter(|diagnostic| {
            ["`psi`", "`c0`", "`cm0`", "`J`", "psi,", "c0,", "cm0,"]
                .iter()
                .any(|name| diagnostic.message.contains(name))
        })
        .map(|diagnostic| diagnostic.message.clone())
        .collect();
    assert!(stale.is_empty(), "{stale:?}");
}

#[test]
fn changing_the_point_never_trips_on_the_saved_eye() {
    for (name, value) in [("Q", "150 m3/h"), ("Q", "100 m3/h"), ("H", "46 m")] {
        let mut project = saved();
        match name {
            "Q" => project.operating.q = value.to_string(),
            _ => project.operating.h = value.to_string(),
        }
        recalc(project, false).unwrap_or_else(|error| panic!("{value}: {error}"));
    }
}

#[test]
fn a_locked_eye_over_the_limit_is_still_an_error() {
    let mut project = saved();
    project.params.insert(
        "c0".to_string(),
        Param::Locked {
            value: "1.5 m/s".to_string(),
        },
    );
    let error = recalc(project, false).unwrap_err().to_string();
    assert!(error.contains("75.0 mm"), "{error}");
}

#[test]
fn a_locked_eye_that_fits_is_kept() {
    let mut project = saved();
    project.params.insert(
        "c0".to_string(),
        Param::Locked {
            value: "12 m/s".to_string(),
        },
    );
    let project = recalc(project, false).unwrap().project;
    let case = case_from_project(&project).unwrap();
    assert_eq!(case.design.c0, Some(12.0));
}

fn locked_value(project: &Project, name: &str) -> Param {
    let Some(Param::Manual { value }) = project.params.get(name) else {
        panic!("{name} no es manual");
    };
    Param::Locked {
        value: value.clone(),
    }
}

fn eye(project: &Project) -> f64 {
    let case = case_from_project(project).unwrap();
    analyze(&case, CurvePoints::MINIMUM).unwrap().result.d1.si
}

#[test]
fn locking_one_eye_value_keeps_the_other_two() {
    for name in ["c0", "cm0", "J"] {
        let mut project = saved();
        let before = project.params.clone();
        let locked = locked_value(&project, name);
        project.params.insert(name.to_string(), locked);

        let case = case_from_project(&project).unwrap();
        assert!(case.design.c0.is_some(), "{name}");
        assert!(case.design.cm0.is_some(), "{name}");
        assert!(case.design.j.is_some(), "{name}");

        let after = recalc(project, false).unwrap().project;
        for other in ["c0", "cm0", "J"] {
            if other != name {
                assert_eq!(
                    format!("{:?}", after.params.get(other)),
                    format!("{:?}", before.get(other)),
                    "{name}: {other}"
                );
            }
        }
    }
}

#[test]
fn locking_the_eye_as_saved_keeps_the_eye() {
    let project = saved();
    let reference = eye(&project);
    let mut locked = project.clone();
    let value = locked_value(&project, "c0");
    locked.params.insert("c0".to_string(), value);
    assert!((eye(&locked) - reference).abs() < 1e-9);
}

fn double_saved() -> Project {
    let mut project = project();
    let casing = project.casing.as_mut().unwrap();
    casing.channels = 2;
    casing.wrap = "180 deg".to_string();
    recalc(project, false).unwrap().project
}

fn text(project: &Project, name: &str) -> String {
    match project.params.get(name) {
        Some(Param::Manual { value } | Param::Locked { value }) => value.clone(),
        other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn the_file_says_what_the_casing_says() {
    let project = double_saved();
    assert_eq!(text(&project, "volute_channels"), "2");
    assert_eq!(text(&project, "volute_wrap_deg"), "180 deg");
    let k3: f64 = text(&project, "k3").parse().unwrap();
    assert!((k3 - 2.0 * 111.0 / 210.0).abs() < 1e-9);
    let twice = recalc(project.clone(), false).unwrap();
    assert_eq!(project.to_toml().unwrap(), twice.project.to_toml().unwrap());
    assert!(twice.changes.is_empty(), "{:?}", twice.changes);
}

#[test]
fn a_casing_added_to_a_saved_project_wins_over_its_volute() {
    let mut project = recalc(Project::new("demo", PumpType::EndSuction), false)
        .unwrap()
        .project;
    project.casing = Some(Casing {
        channels: 2,
        wrap: "180 deg".to_string(),
        ..casing()
    });
    let project = recalc(project, false).unwrap().project;
    assert_eq!(text(&project, "volute_channels"), "2");
}

#[test]
fn a_locked_volute_that_disagrees_is_an_error() {
    let mut project = double_saved();
    project.params.insert(
        "volute_channels".to_string(),
        Param::Locked {
            value: "1".to_string(),
        },
    );
    let error = recalc(project, false).unwrap_err().to_string();
    assert!(error.contains("[casing]"), "{error}");
}

#[test]
fn a_locked_wrap_equal_to_the_casing_passes_after_the_unit_round_trip() {
    let mut project = project();
    let casing = project.casing.as_mut().unwrap();
    casing.channels = 2;
    casing.wrap = "178.7 deg".to_string();
    let mut project = recalc(project, false).unwrap().project;
    project.params.insert(
        "volute_wrap_deg".to_string(),
        Param::Locked {
            value: "178.7 deg".to_string(),
        },
    );
    recalc(project, false).unwrap();
}

#[test]
fn a_locked_throat_ratio_contradicts_the_casing() {
    let mut project = saved();
    project.params.insert(
        "volute_ratio".to_string(),
        Param::Locked {
            value: "0.7".to_string(),
        },
    );
    let error = recalc(project, false).unwrap_err().to_string();
    assert!(error.contains("[casing]"), "{error}");
}

#[test]
fn a_saved_throat_ratio_is_not_a_contradiction() {
    let project = saved();
    assert!(matches!(
        project.params.get("volute_ratio"),
        Some(Param::Manual { .. })
    ));
    assert!(recalc(project, false).is_ok());
}

#[test]
fn a_locked_phi_survives_reselecting_and_the_rest_follows_it() {
    let mut project = double_saved();
    let old_beta2 = text(&project, "beta2");
    project.params.insert(
        "phi".to_string(),
        Param::Locked {
            value: "0.0785".to_string(),
        },
    );
    let reselected = recalc(project, true).unwrap();
    let project = reselected.project;
    assert_eq!(text(&project, "phi"), "0.0785");
    assert!(matches!(
        project.params.get("phi"),
        Some(Param::Locked { .. })
    ));
    assert_ne!(text(&project, "beta2"), old_beta2);
    let z: u32 = text(&project, "blade_count").parse().unwrap();
    assert_eq!(z % 2, 1);
    let found: Vec<_> = reselected
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    assert!(!found.contains(&"HM-DES-019"), "{found:?}");
}

#[test]
fn a_manual_phi_does_not_survive_reselecting() {
    let mut project = double_saved();
    project.params.insert(
        "phi".to_string(),
        Param::Manual {
            value: "0.0785".to_string(),
        },
    );
    let project = recalc(project, true).unwrap().project;
    assert_ne!(text(&project, "phi"), "0.0785");
}

#[test]
fn fit_warnings_reach_audit_and_report_once() {
    let project = saved();
    let audited = audit(&project);
    let reported = report(&project, CurvePoints::MINIMUM).unwrap();
    for code in ["HM-DES-042", "HM-DES-044"] {
        let in_audit = audited.iter().filter(|item| item.code == code).count();
        let in_report = reported
            .diagnostics
            .iter()
            .filter(|item| item.code == code)
            .count();
        assert_eq!((in_audit, in_report), (1, 1), "{code}");
    }
}

fn fitted_case() -> hm_engine::Case {
    case_from_project(&project()).unwrap()
}

#[test]
fn a_new_project_keeps_the_casing_as_it_was_written() {
    let case = fitted_case();
    let analysis = analyze(&case, CurvePoints::MINIMUM).unwrap();
    let project = new_casing_project("rodete", &case, &analysis, &casing()).unwrap();
    assert_eq!(project.casing, Some(casing()));
    assert!(
        project
            .to_toml()
            .unwrap()
            .contains("neck_diameter = \"90 mm\"")
    );

    // La primera vez `rho` deja su redondeo, con o sin carcasa. Después es estable.
    let once = recalc(project, false).unwrap();
    assert!(once.changes.is_empty(), "{:?}", once.changes);
    let twice = recalc(once.project.clone(), false).unwrap();
    assert_eq!(
        once.project.to_toml().unwrap(),
        twice.project.to_toml().unwrap()
    );
    assert_eq!(twice.project.casing, Some(casing()));
}

#[test]
fn a_new_project_cannot_lose_or_change_the_casing() {
    let case = fitted_case();
    let analysis = analyze(&case, CurvePoints::MINIMUM).unwrap();
    let error = new_project("rodete", &case, &analysis).unwrap_err();
    assert_eq!(error.code(), Some("HM-PROJ-030"));

    let other = Casing {
        b3: "31 mm".to_string(),
        ..casing()
    };
    let error = new_casing_project("rodete", &case, &analysis, &other).unwrap_err();
    assert_eq!(error.code(), Some("HM-PROJ-030"));
}

#[test]
fn the_report_gives_the_casing_and_not_the_model_volute() {
    let project = saved();
    let report = report(&project, CurvePoints::MINIMUM).unwrap();
    let casing = report.casing.as_ref().unwrap();
    assert_eq!(casing.d2.value, 0.21);
    assert_eq!(casing.neck_diameter.value, 0.09);
    assert_eq!(casing.channels, 1);
    assert_eq!(casing.wrap, None);
    assert_eq!(casing.throat_area, None);

    assert!(report.volute_sections.is_empty());
    for name in ["a360", "d360", "r4"] {
        assert!(!report.results.contains_key(name), "{name}");
    }
    assert!(report.geometry.unwrap().volute_outlet.is_none());
}

#[test]
fn update_point_keeps_the_casing() {
    let original = saved();
    let mut case = case_from_project(&original).unwrap();
    case.duty.h = hm_engine::Entered::parse("46 m").unwrap();
    let analysis = analyze(&case, CurvePoints::MINIMUM).unwrap();
    let updated = hm_engine::update_point(original.clone(), &case, &analysis).unwrap();
    assert_eq!(updated.casing, original.casing);
}
