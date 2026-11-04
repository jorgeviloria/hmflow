//! `open_project` filtra lo que la ventana sabe editar; `update_point`
//! reescribe el punto en un proyecto que ya existe, sin tocar sus modos.

use hm_engine::{
    Analysis, Case, CaseField, CurvePoints, Density, DensitySource, EditorError, EngineError,
    Entered, Intake, METHOD, PumpType, analyze, audit, case_from_project, open_project,
    parse_temperature, update_point, water_density_at,
};
use hm_project::{Param, Project};

fn fixture(name: &str) -> Project {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hm-project/tests/fixtures")
        .join(name);
    Project::load(&path).unwrap()
}

fn v09() -> Project {
    fixture("end_suction_v0.9.hmf")
}

fn entered(text: &str) -> Entered {
    Entered::parse(text).unwrap()
}

fn analyzed(case: &Case) -> Analysis {
    analyze(case, CurvePoints::new(11).unwrap()).unwrap()
}

fn codes(diagnostics: &hm_core::Diagnostics) -> Vec<String> {
    diagnostics.iter().map(|d| d.code.clone()).collect()
}

fn manual(project: &Project, name: &str) -> String {
    match &project.params[name] {
        Param::Manual { value } | Param::Locked { value } => value.clone(),
        other => panic!("`{name}` no es manual ni locked: {other:?}"),
    }
}

fn opened_case(project: &Project) -> Case {
    case_from_project(project).unwrap()
}

fn unsupported_reason(error: EditorError) -> String {
    match error {
        EditorError::Unsupported { reason } => reason,
        other => panic!("se esperaba Unsupported y salió {other:?}"),
    }
}

fn invalid_codes(error: EditorError) -> Vec<String> {
    match error {
        EditorError::Invalid { diagnostics } => codes(&diagnostics),
        other => panic!("se esperaba Invalid y salió {other:?}"),
    }
}

// ------------------------------------------------------------ update_point

#[test]
fn update_point_rewrites_the_point_and_keeps_the_q_text() {
    let project = v09();
    let mut case = opened_case(&project);
    case.duty.q = entered("1060 gpm");
    case.duty.h = entered("83 m");
    case.duty.n = entered("1750 rpm");
    let analysis = analyzed(&case);

    let updated = update_point(project, &case, &analysis).unwrap();

    assert_eq!(updated.operating.q, "1060 gpm");
    assert_eq!(updated.operating.h, "83 m");
    assert_eq!(updated.operating.n, "1750 rpm");
    assert_eq!(updated.operating.fluid, "water");
}

#[test]
fn a_temperature_change_rewrites_a_manual_rho() {
    let project = v09();
    let mut case = opened_case(&project);
    let temperature = parse_temperature("40 degC").unwrap();
    case.fluid.density = Density {
        value: water_density_at(&temperature).unwrap(),
        source: DensitySource::WaterAtTemperature,
    };
    case.fluid.temperature = Some(temperature);
    let analysis = analyzed(&case);

    let updated = update_point(project, &case, &analysis).unwrap();

    assert_eq!(updated.operating.temperature, "40 degC");
    let rho = manual(&updated, "rho");
    assert_eq!(rho, format!("{:.3} kg/m3", analysis.input.rho.si));
    assert_ne!(rho, "998.204 kg/m3");
    assert!(matches!(updated.params["rho"], Param::Manual { .. }));
}

#[test]
fn a_locked_rho_and_eta_m_are_not_touched() {
    let mut project = v09();
    let rho = manual(&project, "rho");
    let eta_m = manual(&project, "eta_m");
    project
        .params
        .insert("rho".into(), Param::Locked { value: rho.clone() });
    project.params.insert(
        "eta_m".into(),
        Param::Locked {
            value: eta_m.clone(),
        },
    );
    let mut case = opened_case(&project);
    case.duty.q = entered("100 m3/h");
    let analysis = analyzed(&case);

    let updated = update_point(project, &case, &analysis).unwrap();

    assert!(matches!(&updated.params["rho"], Param::Locked { value } if *value == rho));
    assert!(matches!(&updated.params["eta_m"], Param::Locked { value } if *value == eta_m));
}

#[test]
fn an_override_rho_or_eta_m_is_an_input_error() {
    for name in ["rho", "eta_m"] {
        let project = v09();
        let case = opened_case(&project);
        let analysis = analyzed(&case);
        let mut project = project;
        project.params.insert(
            name.into(),
            Param::Override {
                expr: "1".into(),
                author: "yo".into(),
                date: "2026-11-04".into(),
                reason: "prueba".into(),
            },
        );

        let error = update_point(project, &case, &analysis).unwrap_err();

        match error {
            EngineError::Input { field, diagnostic } => {
                assert_eq!(diagnostic.code, "HM-PROJ-019", "{name}");
                assert!(
                    matches!(field, CaseField::Param(param) if param == name),
                    "{name}: {field:?}"
                );
            }
            other => panic!("{name}: se esperaba Input y salió {other:?}"),
        }
    }
}

#[test]
fn written_npsha_and_installation_exclude_each_other() {
    let project = v09();
    let mut case = opened_case(&project);
    case.intake = Intake::Installation {
        pressure: entered("1 atm"),
        height: None,
        losses: None,
        vapor_pressure: None,
    };
    let analysis = analyzed(&case);
    let installed = update_point(project, &case, &analysis).unwrap();
    let suction = installed.suction.as_ref().expect("[suction] escrito");
    assert_eq!(suction.pressure, "1 atm");
    assert!(suction.height.is_empty());
    assert!(installed.operating.npsha.is_empty());

    let mut case = opened_case(&v09());
    case.intake = Intake::Available(entered("12 m"));
    let analysis = analyzed(&case);
    let written = update_point(installed, &case, &analysis).unwrap();
    assert!(written.suction.is_none());
    assert_eq!(written.operating.npsha, "12 m");
}

#[test]
fn update_point_keeps_modes_method_and_other_params() {
    let project = v09();
    let before = project.clone();
    let mut case = opened_case(&project);
    case.duty.q = entered("100 m3/h");
    let analysis = analyzed(&case);

    let updated = update_point(project, &case, &analysis).unwrap();

    assert_eq!(updated.project.name, before.project.name);
    assert_eq!(updated.design.method, METHOD);
    assert_eq!(updated.design.stages, before.design.stages);
    assert_eq!(updated.design.standard, before.design.standard);
    for (name, param) in &before.params {
        if name == "rho" || name == "eta_m" {
            continue;
        }
        assert_eq!(
            format!("{param:?}"),
            format!("{:?}", updated.params[name]),
            "`{name}` cambió"
        );
    }
    assert_eq!(updated.params.len(), before.params.len());
}

#[test]
fn update_point_rejects_a_case_that_is_not_the_analyzed_one() {
    let project = v09();
    let case = opened_case(&project);
    let analysis = analyzed(&case);
    let mut other = case;
    other.duty.q = entered("200 m3/h");

    let error = update_point(project, &other, &analysis).unwrap_err();

    assert!(
        matches!(
            error,
            EngineError::Input {
                field: CaseField::Q,
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn update_point_keeps_a_legacy_method() {
    let project = fixture("end_suction_v0.8.hmf");
    let legacy = project.design.method.clone();
    assert_ne!(legacy, METHOD);
    let mut case = opened_case(&project);
    case.duty.q = entered("100 m3/h");
    let analysis = analyzed(&case);

    let updated = update_point(project, &case, &analysis).unwrap();

    assert_eq!(updated.design.method, legacy, "migrar es cosa de `recalc`");
}

// ------------------------------------------------------------ open_project

#[test]
fn opens_the_v0_9_fixture_with_one_audit_list() {
    let project = v09();

    let opened = open_project(&project).unwrap();

    assert_eq!(opened.case.duty.q.text, "120 m3/h");
    assert_eq!(
        opened.diagnostics.into_vec(),
        audit(&project).into_vec(),
        "la lista es la de `audit`, una sola vez"
    );
}

#[test]
fn opens_the_v0_8_fixture_with_the_legacy_notice() {
    let project = fixture("end_suction_v0.8.hmf");

    let opened = open_project(&project).unwrap();

    let first = opened.diagnostics.iter().next().expect("al menos el aviso");
    assert_eq!(first.code, "HM-PROJ-025");
    assert_eq!(opened.case.duty.q.text, project.operating.q);
}

#[test]
fn water_without_rho_opens() {
    let mut project = v09();
    project.params.remove("rho");

    let opened = open_project(&project).unwrap();

    assert!(matches!(
        opened.case.fluid.density.source,
        DensitySource::WaterAtTemperature
    ));
}

#[test]
fn rejects_oil() {
    let mut project = v09();
    project.operating.fluid = "aceite".into();

    let reason = unsupported_reason(open_project(&project).unwrap_err());

    assert_eq!(reason, "esta interfaz solo edita proyectos de agua");
}

#[test]
fn rejects_a_drifted_rho() {
    let mut project = v09();
    project.params.insert(
        "rho".into(),
        Param::Manual {
            value: "1100 kg/m3".into(),
        },
    );

    let reason = unsupported_reason(open_project(&project).unwrap_err());

    assert_eq!(
        reason,
        "esta interfaz solo edita agua con la densidad de su temperatura"
    );
}

#[test]
fn rejects_a_written_viscosity() {
    let mut project = v09();
    project.operating.viscosity = "5 cSt".into();

    let reason = unsupported_reason(open_project(&project).unwrap_err());

    assert_eq!(reason, "esta interfaz no edita una viscosidad escrita");
}

#[test]
fn rejects_other_pump_types() {
    let mut project = v09();
    project.design.pump_type = PumpType::Inline;
    // ISO 2858 es de end-suction: la validación la rechazaría antes que el tipo.
    project.design.standard.clear();
    project.design.method.clear();

    let reason = unsupported_reason(open_project(&project).unwrap_err());

    assert_eq!(
        reason,
        format!(
            "solo hay motor para end-suction; el proyecto es {}",
            PumpType::Inline
        )
    );
}

#[test]
fn rejects_an_unknown_method() {
    let mut project = v09();
    project.design.method = "end_suction_v9.9".into();

    let found = invalid_codes(open_project(&project).unwrap_err());

    assert_eq!(found, ["HM-PROJ-016"]);
}

#[test]
fn rejects_any_override() {
    for name in ["rho", "eta_m", "J"] {
        let mut project = v09();
        project.params.insert(
            name.into(),
            Param::Override {
                expr: "1".into(),
                author: "yo".into(),
                date: "2026-11-04".into(),
                reason: "prueba".into(),
            },
        );

        let found = invalid_codes(open_project(&project).unwrap_err());

        assert_eq!(found, ["HM-PROJ-019"], "{name}");
    }
}

#[test]
fn a_project_with_format_errors_is_invalid() {
    let mut project = v09();
    project.operating.q = "mucho".into();

    let found = invalid_codes(open_project(&project).unwrap_err());

    assert!(!found.is_empty());
}
