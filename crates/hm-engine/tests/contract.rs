//! El informe es el análisis más la auditoría, para los mismos números.

use hm_design::DesignOptions;
use hm_engine::{
    Analysis, Case, CaseField, CurvePoints, Density, DensitySource, Duty, EngineError, Entered,
    Fluid, FluidKind, Intake, PumpType, analyze, audit_with_analysis, case_from_project,
    new_project, parse_temperature, report,
};
use hm_formula::Registry;
use hm_project::{Param, Project};
use hm_units::parse_quantity;

fn entered(text: &str) -> Entered {
    Entered::parse(text).unwrap()
}

fn points() -> CurvePoints {
    CurvePoints::new(11).unwrap()
}

fn duty_case(q: &str, h: &str, n: &str) -> Case {
    Case {
        pump_type: PumpType::EndSuction,
        duty: Duty {
            q: entered(q),
            h: entered(h),
            n: entered(n),
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
        design: DesignOptions::default(),
    }
}

fn b1() -> Case {
    duty_case("1060 gpm", "83 m", "1750 rpm")
}

/// D1.A1 con las selecciones publicadas y ηm = 0,96.
fn d1a1() -> Case {
    let mut case = duty_case("0.016 m3/s", "13.8 m", "1800 rpm");
    case.mechanical_efficiency = Some(0.96);
    case.design = DesignOptions {
        psi: Some(0.87),
        phi: Some(0.13),
        beta2_rad: Some(23.5_f64.to_radians()),
        blade_count: Some(7),
        c0: Some(3.6),
        cm0: Some(2.25),
        j: Some(1.03),
        eta_v: Some(0.9722),
        ..DesignOptions::default()
    };
    case
}

fn fixture(name: &str) -> Project {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hm-project/tests/fixtures")
        .join(name);
    Project::load(&path).unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
}

/// El informe publica el mismo análisis, no un diseño calculado aparte.
fn assert_agrees(report: &hm_engine::DesignReport, analysis: &Analysis) {
    close(report.results["eta"].value, analysis.result.eta);
    close(report.results["d2"].value, analysis.result.d2.si);
    close(
        report.selection["blade_count"].value,
        f64::from(analysis.result.selection.blade_count),
    );
    assert_eq!(report.curve.len(), analysis.curve.len());
    for (point, expected) in report.curve.iter().zip(&analysis.curve) {
        close(point.q.value, expected.q.si);
        close(point.h.value, expected.h.si);
        close(point.eta.value, expected.eta);
        close(point.p_shaft.value, expected.p_shaft.si);
        close(point.npshr.value, expected.npshr.si);
    }

    let published = hm_design::DesignReport::new(
        &analysis.input,
        &analysis.result,
        report.curve.len(),
        Vec::new(),
    );
    let published = match &analysis.geometry {
        Ok(geometry) => published.with_geometry(geometry),
        Err(_) => published,
    };
    let published = match &analysis.mechanics {
        Ok(mechanics) => published.with_mechanics(mechanics),
        Err(_) => published,
    };
    let published = match &analysis.viscous {
        Ok(Some(correction)) => published.with_viscous(correction),
        _ => published,
    };
    assert_eq!(report.geometry, published.geometry);
    assert_eq!(report.mechanics, published.mechanics);
    assert_eq!(report.viscous, published.viscous);
    assert_eq!(report.curve, published.curve);

    // La auditoría puede añadir HM-DES-013 si un manual escrito no es el que
    // el motor volvería a elegir. Los avisos del análisis van delante.
    let avisos = |diagnostics: &[hm_core::Diagnostic]| {
        diagnostics
            .iter()
            .filter(|diagnostic| !diagnostic.is_error())
            .map(|diagnostic| diagnostic.code.clone())
            .collect::<Vec<_>>()
    };
    let report_avisos = avisos(&report.diagnostics);
    let analysis_avisos = avisos(&analysis.diagnostics);
    assert!(
        report_avisos.starts_with(analysis_avisos.as_slice()),
        "informe {report_avisos:?} análisis {analysis_avisos:?}"
    );
    assert!(
        report_avisos[analysis_avisos.len()..]
            .iter()
            .all(|code| code == "HM-DES-013")
    );
}

#[test]
fn report_and_analysis_agree() {
    for case in [b1(), d1a1()] {
        let analysis = analyze(&case, points()).unwrap();
        let project = new_project("contrato", &case, &analysis).unwrap();
        let report = report(&project, points()).unwrap();
        assert_agrees(&report, &analysis);
    }

    let project = fixture("end_suction_v0.9.hmf");
    let analysis = analyze(&case_from_project(&project).unwrap(), points()).unwrap();
    let report = report(&project, points()).unwrap();
    assert_agrees(&report, &analysis);
}

#[test]
fn report_is_analyze_plus_audit() {
    let case = b1();
    let project = new_project("b1", &case, &analyze(&case, points()).unwrap()).unwrap();
    let report = report(&project, points()).unwrap();

    let validation = project.validate(&Registry::all());
    assert!(!validation.has_errors());
    let analysis = analyze(&case_from_project(&project).unwrap(), points()).unwrap();
    let mut expected = validation.into_vec();
    expected.extend(audit_with_analysis(&project, &analysis).into_vec());
    assert_eq!(report.diagnostics, expected);

    let mut seen = std::collections::HashSet::new();
    for diagnostic in &report.diagnostics {
        assert!(
            seen.insert((diagnostic.code.clone(), diagnostic.message.clone())),
            "diagnóstico duplicado {}",
            diagnostic.code
        );
    }

    // El esquema (HM-PROJ-017) lo pone `audit`, no esta composición.
    let mut missing = project.clone();
    missing.params.remove("k_eff");
    let audited = hm_engine::report(&missing, points()).unwrap();
    assert!(
        audited
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "HM-PROJ-017")
    );
    let analysis = analyze(&case_from_project(&missing).unwrap(), points()).unwrap();
    assert_eq!(
        audited.diagnostics,
        audit_with_analysis(&missing, &analysis).into_vec()
    );
}

#[test]
fn report_fails_on_invalid_project_or_method() {
    let mut project = new_project("b1", &b1(), &analyze(&b1(), points()).unwrap()).unwrap();
    project.params.insert(
        "psi".to_string(),
        Param::Override {
            expr: "0.5".to_string(),
            author: "test".to_string(),
            date: "2026-11-03".to_string(),
            reason: "prueba".to_string(),
        },
    );
    let error = report(&project, points()).unwrap_err();
    assert_eq!(error.code(), Some("HM-PROJ-019"));
    assert_eq!(error.field(), Some(CaseField::Param("psi")));

    let error = report(&fixture("end_suction_v0.8.hmf"), points()).unwrap_err();
    assert_eq!(error.code(), Some("HM-PROJ-025"));
    assert!(
        matches!(error, EngineError::LegacyMethod { ref method } if method == "end_suction_v0.8"),
        "{error:?}"
    );

    project.operating.q = "no es un caudal".to_string();
    let error = report(&project, points()).unwrap_err();
    match error {
        EngineError::Invalid { diagnostics } => {
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == "HM-PROJ-002")
            );
        }
        other => panic!("se esperaba Invalid, llegó {other:?}"),
    }
}

#[test]
fn report_includes_audit_warnings() {
    let mut project = new_project("b1", &b1(), &analyze(&b1(), points()).unwrap()).unwrap();
    match project.params.get_mut("d2") {
        Some(Param::Auto { value, .. }) => *value = Some("9 m".to_string()),
        other => panic!("d2 no es auto: {other:?}"),
    }

    let report = report(&project, points()).unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-PROJ-015"),
        "un auto manipulado tiene que viajar en el informe: {:?}",
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
    );
    assert!(report.geometry.is_some());
    assert!(report.mechanics.is_some());
}
