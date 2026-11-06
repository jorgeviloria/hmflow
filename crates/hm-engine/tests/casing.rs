//! `[casing]` a través del motor: lo que el proyecto declara llega al diseño
//! y el ψ guardado no bloquea cambiar el punto.

use hm_engine::{
    CaseField, CurvePoints, DesignEdits, EngineError, analyze, audit, case_from_project,
    design_edits, edited_options, new_casing_project, new_project, open_project, recalc, report,
    update_design,
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
        ..Casing::default()
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

fn stale_warnings(project: &Project) -> Vec<String> {
    audit(project)
        .iter()
        .filter(|diagnostic| diagnostic.code == "HM-DES-013")
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

#[test]
fn a_locked_selection_counts_when_checking_the_others() {
    let mut project = double_saved();
    project.params.insert(
        "phi".to_string(),
        Param::Locked {
            value: "0.12".to_string(),
        },
    );
    let project = recalc(project, true).unwrap().project;
    assert_eq!(stale_warnings(&project), Vec::<String>::new());
}

#[test]
fn a_manual_selection_that_ignores_the_locked_one_is_still_stale() {
    let mut project = double_saved();
    project.params.insert(
        "phi".to_string(),
        Param::Locked {
            value: "0.12".to_string(),
        },
    );
    let mut project = recalc(project, true).unwrap().project;
    let chosen = text(&project, "blade_count");
    let other = if chosen == "7" { "5" } else { "7" };
    project.params.insert(
        "blade_count".to_string(),
        Param::Manual {
            value: other.to_string(),
        },
    );
    let found = stale_warnings(&project);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("blade_count"), "{found:?}");
}

fn with_leakage() -> Casing {
    Casing {
        seal_clearance: "0.3 mm".to_string(),
        seal_length: "15 mm".to_string(),
        rear_seal_diameter: "80 mm".to_string(),
        rear_seal_length: "15 mm".to_string(),
        balance_hole_count: 6,
        balance_hole_diameter: "9 mm".to_string(),
        balance_hole_circle: "50 mm".to_string(),
        ..casing()
    }
}

#[test]
fn the_leakage_data_reaches_the_design_in_si() {
    let mut project = project();
    project.casing = Some(with_leakage());
    let case = case_from_project(&project).unwrap();
    let data = case.design.casing.unwrap().leakage.unwrap();
    let close = |actual: f64, expected: f64| {
        assert!(
            (actual - expected).abs() < 1e-12,
            "{actual} frente a {expected}"
        );
    };
    close(data.seal_clearance, 0.0003);
    close(data.seal_length, 0.015);
    close(data.rear_seal_diameter, 0.08);
    close(data.rear_seal_length, 0.015);
    assert_eq!(data.balance_hole_count, 6);
    close(data.balance_hole_diameter, 0.009);
    close(data.balance_hole_circle, 0.05);
}

#[test]
fn without_leakage_data_the_design_has_none() {
    let case = case_from_project(&project()).unwrap();
    assert_eq!(case.design.casing.unwrap().leakage, None);
}

#[test]
fn half_the_leakage_data_is_an_error_even_without_validating() {
    let mut project = project();
    project.casing = Some(Casing {
        seal_length: String::new(),
        ..with_leakage()
    });
    let error = case_from_project(&project).unwrap_err();
    assert_eq!(error.code(), Some("HM-PROJ-030"));
    assert!(error.to_string().contains("seal_length"), "{error}");
}

#[test]
fn the_report_gives_the_leakage_data() {
    let mut project = project();
    project.casing = Some(with_leakage());
    let project = recalc(project, false).unwrap().project;
    let report = report(&project, CurvePoints::MINIMUM).unwrap();
    let data = report.casing.unwrap().leakage.unwrap();
    assert!((data.rear_seal_diameter.value - 0.08).abs() < 1e-12);
    assert_eq!(data.balance_hole_count, 6);
}

fn saved_with_leakage() -> Project {
    let mut project = project();
    project.casing = Some(with_leakage());
    recalc(project, false).unwrap().project
}

#[test]
fn with_leakage_data_recalculating_is_stable() {
    // La primera pasada deja `rho` redondeada, con o sin datos de fuga.
    let once = recalc(saved_with_leakage(), false).unwrap();
    assert!(once.changes.is_empty(), "{:?}", once.changes);
    let twice = recalc(once.project.clone(), false).unwrap();
    assert_eq!(
        once.project.to_toml().unwrap(),
        twice.project.to_toml().unwrap()
    );
}

#[test]
fn with_leakage_data_a_new_point_recalculates_eta_v() {
    let mut project = saved_with_leakage();
    let eta_v = |project: &Project| {
        let case = case_from_project(project).unwrap();
        analyze(&case, CurvePoints::MINIMUM).unwrap().result.eta_v
    };
    let before = eta_v(&project);
    project.operating.q = "150 m3/h".to_string();
    let project = recalc(project, false).unwrap().project;
    assert!((eta_v(&project) - before).abs() > 1e-4);
}

#[test]
fn a_new_project_with_leakage_data_can_be_written() {
    let mut base = project();
    base.casing = Some(with_leakage());
    let case = case_from_project(&base).unwrap();
    let analysis = analyze(&case, CurvePoints::MINIMUM).unwrap();
    assert!(new_casing_project("rodete", &case, &analysis, &with_leakage()).is_ok());
}

#[test]
fn the_report_gives_the_rear_leakage_only_with_leakage_data() {
    let plain = report(&saved(), CurvePoints::MINIMUM).unwrap();
    assert!(plain.hydraulics.unwrap().rear_seal_leakage_ratio.is_none());
    let full = report(&saved_with_leakage(), CurvePoints::MINIMUM).unwrap();
    let ratio = full.hydraulics.unwrap().rear_seal_leakage_ratio.unwrap();
    assert!(ratio.value > 0.0);
}

#[test]
fn a_negative_axial_thrust_reaches_the_report_and_the_seal_too() {
    let mut project = project();
    project.casing = Some(Casing {
        neck_diameter: "90 mm".to_string(),
        rear_seal_diameter: "150 mm".to_string(),
        balance_hole_circle: "80 mm".to_string(),
        shaft_seal_diameter: "28 mm".to_string(),
        ..with_leakage()
    });
    let project = recalc(project, false).unwrap().project;
    let mechanics = report(&project, CurvePoints::MINIMUM)
        .unwrap()
        .mechanics
        .unwrap();
    assert!(mechanics.axial_thrust_low.value < 0.0);
    assert!(mechanics.axial_thrust_low.value <= mechanics.axial_thrust_high.value);
    assert!((mechanics.shaft_seal_diameter.value - 0.028).abs() < 1e-12);
}

fn eye_locked(name: &str) -> Project {
    let mut project = saved();
    let locked = locked_value(&project, name);
    project.params.insert(name.to_string(), locked);
    project
}

#[test]
fn reselecting_keeps_the_whole_eye_when_one_value_is_locked() {
    for name in ["c0", "cm0", "J"] {
        let project = eye_locked(name);
        let before = project.params.clone();
        let after = recalc(project, true).unwrap().project;
        for other in ["c0", "cm0", "J"] {
            assert_eq!(
                format!("{:?}", after.params.get(other)),
                format!("{:?}", before.get(other)),
                "{name}: {other}"
            );
        }
    }
}

#[test]
fn a_locked_eye_is_not_reported_as_stale() {
    for name in ["c0", "cm0", "J"] {
        assert_eq!(
            stale_warnings(&eye_locked(name)),
            Vec::<String>::new(),
            "{name}"
        );
    }
}

#[test]
fn a_locked_eye_as_saved_passes_after_the_first_pass_too() {
    // Con datos de fuga, la primera pasada usa el ηv estadístico: el ojo bloqueado
    // no debe rechazarse por una diferencia que al converger desaparece.
    let mut project = saved_with_leakage();
    let locked = locked_value(&project, "c0");
    project.params.insert("c0".to_string(), locked);
    recalc(project, false).unwrap();
}

#[test]
fn the_model_volute_is_not_saved_and_an_older_file_still_passes() {
    let fresh = saved();
    for name in ["a360", "d360", "r4"] {
        assert!(!fresh.params.contains_key(name), "{name}");
    }
    assert!(!audit(&fresh).has_errors());

    // Un archivo guardado antes los traía como `auto`: se acepta, y `recalc` los quita.
    let mut older = saved();
    older.params.insert(
        "a360".to_string(),
        Param::Auto {
            formula: "design.volute_throat_area_ratio".to_string(),
            value: Some("0.1 m2".to_string()),
        },
    );
    assert!(!audit(&older).has_errors());
    let recalculated = recalc(older, false).unwrap();
    assert!(!recalculated.project.params.contains_key("a360"));
}

#[test]
fn without_a_casing_the_model_volute_is_still_required() {
    let mut free = saved();
    free.casing = None;
    let mut free = recalc(free, false).unwrap().project;
    assert!(free.params.contains_key("a360"));
    free.params.remove("a360");
    assert!(
        audit(&free)
            .iter()
            .any(|item| item.code == "HM-PROJ-017" && item.message.contains("a360"))
    );
}

#[test]
fn a_fixed_eye_does_not_make_the_blade_count_look_stale() {
    // Ojo grande frente a D2: ahí el ojo mueve el deslizamiento, y con él Z. La
    // comparación tiene que partir del ojo entero, no del único valor bloqueado.
    let mut wide = project();
    let casing = wide.casing.as_mut().unwrap();
    casing.d2 = "160 mm".to_string();
    casing.neck_diameter = "140 mm".to_string();
    casing.eye_diameter_max = "120 mm".to_string();
    casing.hub_diameter = "50 mm".to_string();
    casing.cutwater_radius = "85 mm".to_string();
    casing.b3 = "45 mm".to_string();
    wide.operating.q = "480 m3/h".to_string();
    wide.operating.h = "18 m".to_string();
    let mut wide = recalc(wide, false).unwrap().project;
    let locked = locked_value(&wide, "c0");
    wide.params.insert("c0".to_string(), locked);
    let stale: Vec<_> = audit(&wide)
        .iter()
        .filter(|item| item.code == "HM-DES-013")
        .map(|item| item.message.clone())
        .collect();
    assert!(stale.is_empty(), "{stale:?}");
}

#[test]
fn without_a_casing_the_locked_value_still_drives_the_comparison() {
    let mut stale = 0;
    for c0 in [1.5, 2.0, 2.5, 3.0, 3.5, 4.0] {
        for (q, h) in [
            ("480 m3/h", "18 m"),
            ("300 m3/h", "12 m"),
            ("120 m3/h", "45 m"),
        ] {
            let mut free = Project::new("demo", PumpType::EndSuction);
            free.operating.q = q.to_string();
            free.operating.h = h.to_string();
            free.params.insert(
                "c0".to_string(),
                Param::Locked {
                    value: format!("{c0} m/s"),
                },
            );
            let Ok(done) = recalc(free, false) else {
                continue;
            };
            stale += audit(&done.project)
                .iter()
                .filter(|item| item.code == "HM-DES-013")
                .count();
        }
    }
    assert_eq!(stale, 0);
}

#[test]
fn holes_that_do_not_fit_are_a_casing_problem() {
    let mut bad = project();
    let casing = bad.casing.as_mut().unwrap();
    casing.seal_clearance = "0.3 mm".to_string();
    casing.seal_length = "15 mm".to_string();
    casing.rear_seal_diameter = "80 mm".to_string();
    casing.rear_seal_length = "15 mm".to_string();
    casing.balance_hole_count = 6;
    casing.balance_hole_diameter = "9 mm".to_string();
    casing.balance_hole_circle = "300 mm".to_string();
    let found = audit(&bad);
    assert!(found.iter().any(|item| item.code == "HM-PROJ-030"));
    assert!(found.iter().all(|item| item.code != "HM-PROJ-014"));
}

#[test]
fn a_rotor_that_does_not_fit_points_at_the_casing_not_the_density() {
    let mut tight = saved();
    tight.params.insert(
        "phi".to_string(),
        Param::Locked {
            value: "0.02".to_string(),
        },
    );
    let found = audit(&tight);
    let error = found
        .iter()
        .find(|item| item.code == "HM-PROJ-014")
        .unwrap();
    assert!(error.message.contains("no cabe"), "{}", error.message);
    let hint = error.hint.as_deref().unwrap();
    assert!(hint.contains("[casing]"), "{hint}");
    assert!(!hint.contains("densidad"), "{hint}");
}

#[test]
fn a_selection_that_cannot_be_checked_says_so() {
    // b3 justo: al bajar H, el φ del motor pide un b2 que no cabe.
    let mut narrow = project();
    narrow.casing.as_mut().unwrap().b3 = "22 mm".to_string();
    let mut low = recalc(narrow, false).unwrap().project;
    low.operating.h = "22 m".to_string();
    assert!(recalc(low.clone(), true).is_err());
    let found = audit(&low);
    let stale = found.iter().find(|item| item.code == "HM-DES-013").unwrap();
    assert!(stale.message.contains("contrastar"), "{}", stale.message);
    assert!(stale.hint.as_deref().unwrap().contains("--reselect"));
}

fn edited(project: &Project, edit: impl FnOnce(&mut DesignEdits)) -> Project {
    let mut edits = design_edits(project);
    edit(&mut edits);
    update_design(project.clone(), &edits).unwrap().project
}

fn is_locked_to(project: &Project, name: &str, expected: &str) -> bool {
    matches!(project.params.get(name), Some(Param::Locked { value }) if value == expected)
}

#[test]
fn editing_nothing_leaves_the_file_as_it_was() {
    let project = saved();
    assert_eq!(
        edited(&project, |_| ()).to_toml().unwrap(),
        project.to_toml().unwrap()
    );
}

#[test]
fn an_edit_of_the_casing_is_kept_after_reopening() {
    let project = edited(&saved(), |edits| {
        edits.casing.as_mut().unwrap().b3 = "28 mm".to_string();
    });
    let reopened = Project::from_toml(&project.to_toml().unwrap()).unwrap();
    assert_eq!(reopened.casing.unwrap().b3, "28 mm");
    assert!(!audit(&project).has_errors());
}

#[test]
fn fixing_phi_and_z_locks_them_and_redoes_what_depends_on_them() {
    let before = saved();
    let project = edited(&before, |edits| {
        edits.phi = "0.08".to_string();
        edits.blade_count = "5".to_string();
    });
    assert!(is_locked_to(&project, "phi", "0.08"));
    assert!(is_locked_to(&project, "blade_count", "5"));
    assert_ne!(
        format!("{:?}", project.params.get("beta2")),
        format!("{:?}", before.params.get("beta2"))
    );
    let found = audit(&project);
    assert!(found.iter().all(|item| item.code != "HM-DES-019"));
    assert!(found.iter().all(|item| item.code != "HM-DES-013"));
}

#[test]
fn releasing_leaves_the_file_as_if_it_had_never_been_fixed() {
    let never = saved();
    let fixed = edited(&never, |edits| {
        edits.phi = "0.08".to_string();
        edits.blade_count = "5".to_string();
    });
    let released = edited(&fixed, |edits| {
        edits.phi.clear();
        edits.blade_count.clear();
    });
    assert_eq!(released.to_toml().unwrap(), never.to_toml().unwrap());
}

#[test]
fn the_box_shows_only_what_is_a_decision() {
    let project = saved();
    let edits = design_edits(&project);
    assert_eq!(edits.phi, "");
    assert_eq!(edits.blade_count, "");
    assert_eq!(edits.roughness, "");
    assert_eq!(edits.incidence, "");
    let fixed = edited(&project, |edits| edits.phi = "0.08".to_string());
    assert_eq!(design_edits(&fixed).phi, "0.08");
}

#[test]
fn a_fixed_eye_survives_a_fixed_phi() {
    let mut project = saved();
    let locked = locked_value(&project, "c0");
    project.params.insert("c0".to_string(), locked.clone());
    let reference = eye(&project);
    let done = edited(&project, |edits| edits.phi = "0.08".to_string());
    assert_eq!(
        format!("{:?}", done.params.get("c0")),
        format!("{:?}", Some(&locked))
    );
    assert!((eye(&done) / reference - 1.0).abs() < 1e-9);
}

#[test]
fn the_measures_are_written_and_emptying_them_goes_back_to_the_default() {
    let project = saved();
    let measured = edited(&project, |edits| {
        edits.roughness = "0.05 mm".to_string();
        edits.incidence = "4 deg".to_string();
    });
    assert_eq!(design_edits(&measured).roughness, "0.05 mm");
    assert_eq!(design_edits(&measured).incidence, "4 deg");
    assert!(matches!(
        measured.params.get("incidence_deg"),
        Some(Param::Manual { value }) if value == "4 deg"
    ));
    let back = edited(&measured, |edits| {
        edits.roughness.clear();
        edits.incidence.clear();
    });
    assert_eq!(back.to_toml().unwrap(), project.to_toml().unwrap());
}

#[test]
fn removing_the_casing_goes_back_to_the_free_design() {
    let double = double_saved();
    let free = edited(&double, |edits| edits.casing = None);
    assert_eq!(free.casing, None);
    assert!(free.params.contains_key("a360"));
    let case = case_from_project(&free).unwrap();
    assert_ne!(case.design.volute_channels, Some(2));
    assert!(!audit(&free).has_errors());
}

#[test]
fn a_bad_box_names_its_own_field() {
    let project = saved();
    let mut edits = design_edits(&project);
    edits.casing.as_mut().unwrap().neck_diameter = "mucho".to_string();
    let Err(EngineError::Input { field, diagnostic }) = update_design(project.clone(), &edits)
    else {
        panic!("debía ser un error de entrada");
    };
    assert_eq!(field, CaseField::Casing("neck_diameter"));
    assert_eq!(diagnostic.code, "HM-PROJ-030");

    let mut edits = design_edits(&project);
    edits.casing.as_mut().unwrap().neck_diameter = "70 mm".to_string();
    let Err(EngineError::Input { field, .. }) = update_design(project, &edits) else {
        panic!("debía ser un error de entrada");
    };
    assert_eq!(field, CaseField::Casing("eye_diameter_max"));
}

fn beta2_text(project: &Project) -> String {
    format!("{:?}", project.params.get("beta2"))
}

#[test]
fn each_decision_alone_redoes_what_depends_on_it() {
    let before = saved();
    let phi = edited(&before, |edits| edits.phi = "0.08".to_string());
    assert_ne!(beta2_text(&phi), beta2_text(&before));
    let blades = edited(&before, |edits| edits.blade_count = "6".to_string());
    assert_ne!(beta2_text(&blades), beta2_text(&before));
    let casing = edited(&before, |edits| {
        let casing = edits.casing.as_mut().unwrap();
        casing.d2 = "220 mm".to_string();
        casing.cutwater_radius = "116 mm".to_string();
    });
    assert_ne!(beta2_text(&casing), beta2_text(&before));
    assert!(audit(&casing).iter().all(|item| item.code != "HM-DES-019"));
}

#[test]
fn a_measure_does_not_undo_a_selection_written_by_hand() {
    let mut project = saved();
    project.params.insert(
        "phi".to_string(),
        Param::Manual {
            value: "0.07".to_string(),
        },
    );
    let project = recalc(project, false).unwrap().project;
    let measured = edited(&project, |edits| edits.incidence = "4 deg".to_string());
    assert!(matches!(
        measured.params.get("phi"),
        Some(Param::Manual { value }) if value == "0.07"
    ));
}

/// La ventana calcula con `edited_options` y guarda con `update_design`.
fn preview_matches_what_is_saved(project: &Project, edit: impl FnOnce(&mut DesignEdits)) {
    let opened = open_project(project).unwrap();
    let mut edits = opened.edits.clone();
    edit(&mut edits);
    let design = edited_options(
        &opened.case.design,
        &opened.reselected,
        &opened.edits,
        &edits,
    )
    .unwrap();
    let case = hm_engine::Case {
        design,
        ..opened.case
    };
    let shown = analyze(&case, CurvePoints::MINIMUM).unwrap();
    let saved = update_design(project.clone(), &edits).unwrap().project;
    let again = analyze(&case_from_project(&saved).unwrap(), CurvePoints::MINIMUM).unwrap();
    // La marca sale distinta en cualquier archivo: el ojo del motor se relee como
    // `manual`. No es un número del diseño.
    let text = |result: &hm_design::DesignResult| {
        format!("{result:?}").replace("eye_excludes_hub: false", "eye_excludes_hub: true")
    };
    let (shown, again) = (text(&shown.result), text(&again.result));
    let from = shown
        .chars()
        .zip(again.chars())
        .position(|(a, b)| a != b)
        .unwrap_or(0);
    let near = |text: &str| {
        text.chars()
            .skip(from.saturating_sub(40))
            .take(90)
            .collect::<String>()
    };
    assert_eq!(near(&shown), near(&again), "{edits:?}");
    assert_eq!(shown, again);
}

#[test]
fn what_is_shown_is_what_is_saved() {
    preview_matches_what_is_saved(&saved(), |_| ());
    preview_matches_what_is_saved(&saved(), |edits| edits.phi = "0.08".to_string());
    preview_matches_what_is_saved(&saved(), |edits| edits.blade_count = "6".to_string());
    preview_matches_what_is_saved(&saved(), |edits| {
        edits.roughness = "0.05 mm".to_string();
        edits.incidence = "4 deg".to_string();
    });
    preview_matches_what_is_saved(&saved(), |edits| {
        edits.casing.as_mut().unwrap().b3 = "28 mm".to_string();
    });
    preview_matches_what_is_saved(&double_saved(), |edits| edits.casing = None);

    let fixed = edited(&saved(), |edits| edits.phi = "0.08".to_string());
    preview_matches_what_is_saved(&fixed, |edits| edits.phi.clear());
    let measured = edited(&saved(), |edits| edits.incidence = "4 deg".to_string());
    preview_matches_what_is_saved(&measured, |edits| edits.incidence.clear());
}

#[test]
fn a_casing_can_be_added_to_a_free_project() {
    let free = edited(&saved(), |edits| edits.casing = None);
    preview_matches_what_is_saved(&free, |edits| edits.casing = Some(casing()));
}

#[test]
fn a_missing_box_says_it_is_missing() {
    let mut empty = casing();
    empty.b3.clear();
    let error = hm_engine::casing_fit(&empty).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Casing("b3")));
    assert!(error.to_string().contains("hace falta"), "{error}");
}

#[test]
fn half_the_leakage_data_names_what_is_missing() {
    let mut half = casing();
    half.seal_clearance = "0.3 mm".to_string();
    let error = hm_engine::casing_fit(&half).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Casing("balance_hole_count")));
    assert!(error.to_string().contains("hace falta"), "{error}");

    half.balance_hole_count = 6;
    let error = hm_engine::casing_fit(&half).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Casing("seal_length")));
    assert!(error.to_string().contains("hace falta"), "{error}");
}

#[test]
fn a_casing_replaces_a_volute_fixed_by_hand() {
    let mut free = edited(&saved(), |edits| edits.casing = None);
    let value = "1.2".to_string();
    free.params
        .insert("k3".to_string(), Param::Locked { value });
    preview_matches_what_is_saved(&free, |edits| edits.casing = Some(casing()));
}
