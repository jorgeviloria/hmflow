//! `new_project` escribe el caso sin recalcular. `report` no imprime ni guarda.

use hm_design::geometry::BLADE_STEPS;
use hm_design::mechanics::DEFAULT_SHAFT_SHEAR_STRESS;
use hm_design::{DesignError, DesignInput, DesignOptions, design};
use hm_engine::{
    Analysis, Case, CaseField, CurvePoints, Density, DensitySource, Duty, EngineError, Entered,
    Fluid, FluidKind, Intake, METHOD, ManualDefaults, PumpType, Stage, Viscosity, analyze,
    design_params, new_project, parse_temperature, report,
};
use hm_project::{Param, Project};
use hm_units::parse_quantity;

fn entered(text: &str) -> Entered {
    Entered::parse(text).unwrap()
}

fn points() -> CurvePoints {
    CurvePoints::new(11).unwrap()
}

/// Agua a 20 °C, sin aspiración y con las opciones por defecto.
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

/// Punto de catálogo de B1: 1060 gpm, 83 m, 1750 rpm.
fn b1() -> Case {
    duty_case("1060 gpm", "83 m", "1750 rpm")
}

fn analyzed(case: &Case) -> Analysis {
    analyze(case, points()).unwrap()
}

fn manual_text<'a>(project: &'a Project, name: &str) -> &'a str {
    match &project.params[name] {
        Param::Manual { value } | Param::Locked { value } => value,
        Param::Auto {
            value: Some(value), ..
        } => value,
        other => panic!("{name} no guarda texto: {other:?}"),
    }
}

#[test]
fn new_project_writes_the_entered_text_literally() {
    let mut case = b1();
    case.fluid.kind = FluidKind::Custom("aceite".to_string());
    case.fluid.temperature = None;
    case.fluid.viscosity = Some(Viscosity::Kinematic(entered("30 cSt")));
    case.intake = Intake::Installation {
        pressure: entered("1 atm"),
        height: Some(entered("2 m")),
        losses: None,
        vapor_pressure: Some(entered("2000 Pa")),
    };
    let project = new_project("b1", &case, &analyzed(&case)).unwrap();

    assert_eq!(project.operating.q, "1060 gpm");
    assert_eq!(project.operating.h, "83 m");
    assert_eq!(project.operating.n, "1750 rpm");
    assert_eq!(project.operating.fluid, "aceite");
    assert_eq!(project.operating.temperature, "");
    assert_eq!(project.operating.viscosity, "30 cSt");
    assert_eq!(project.operating.npsha, "");
    assert_eq!(project.design.method, METHOD);
    let suction = project
        .suction
        .expect("la instalación se escribe en [suction]");
    assert_eq!(suction.pressure, "1 atm");
    assert_eq!(suction.height, "2 m");
    assert_eq!(suction.losses, "");
    assert_eq!(suction.vapor_pressure, "2000 Pa");

    let mut available = b1();
    available.intake = Intake::Available(entered("15 m"));
    let project = new_project("b1", &available, &analyzed(&available)).unwrap();
    assert_eq!(project.operating.npsha, "15 m");
    assert!(project.suction.is_none());
}

#[test]
fn new_project_always_writes_resolved_rho_and_eta_m() {
    let mut explicit = b1();
    explicit.mechanical_efficiency = Some(0.96);
    let analysis = analyzed(&explicit);
    let project = new_project("b1", &explicit, &analysis).unwrap();
    assert_eq!(manual_text(&project, "rho"), "998.2 kg/m3");
    assert_eq!(
        manual_text(&project, "eta_m"),
        analysis.input.eta_m.to_string()
    );
    assert_eq!(manual_text(&project, "eta_m"), "0.96");

    let mut derived = b1();
    derived.fluid.density.source = DensitySource::WaterAtTemperature;
    let analysis = analyzed(&derived);
    let project = new_project("b1", &derived, &analysis).unwrap();
    assert_eq!(
        manual_text(&project, "rho"),
        format!("{:.3} kg/m3", analysis.input.rho.si)
    );
    assert_ne!(manual_text(&project, "rho"), "998.2 kg/m3");
    assert_eq!(
        manual_text(&project, "eta_m"),
        analysis.input.eta_m.to_string()
    );
}

#[test]
fn new_project_reports_a_failed_stage() {
    let case = b1();
    let mut analysis = analyzed(&case);
    analysis.geometry = Err(DesignError::Input("geometría".into()));
    analysis.mechanics = Err(DesignError::Input("mecánica".into()));
    let error = new_project("b1", &case, &analysis).unwrap_err();
    assert!(
        matches!(
            error,
            EngineError::Stage {
                stage: Stage::Geometry,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.field(), None);

    analysis.geometry = analyzed(&case).geometry;
    analysis.mechanics = analyzed(&case).mechanics;
    analysis.viscous = Err(DesignError::Input("viscosidad".into()));
    let error = new_project("b1", &case, &analysis).unwrap_err();
    assert!(
        matches!(
            error,
            EngineError::Stage {
                stage: Stage::Viscous,
                ..
            }
        ),
        "{error:?}"
    );

    // `Ok(None)` no es un fallo: no había corrección que aplicar.
    let mut clear = analyzed(&case);
    clear.viscous = Ok(None);
    assert!(new_project("b1", &case, &clear).is_ok());
}

#[test]
fn report_of_a_designed_project_matches_its_stored_values() {
    let project = designed_project();
    let report = report(&project, points()).unwrap();

    assert_eq!(report.method, METHOD);
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error())
    );
    let geometry = report
        .geometry
        .as_ref()
        .expect("el informe lleva geometría");
    assert_eq!(geometry.impeller.blade.len(), BLADE_STEPS + 1);
    assert!(
        geometry
            .volute_outlet
            .as_ref()
            .unwrap()
            .nominal_diameter
            .value
            > 0.0
    );
    let mechanics = report
        .mechanics
        .as_ref()
        .expect("el informe lleva mecánica");
    assert_eq!(
        mechanics.shaft_shear_stress.value,
        DEFAULT_SHAFT_SHEAR_STRESS
    );
    assert!(mechanics.shaft_min_diameter.value > 0.0);
    let stored = parse_quantity(manual_text(&project, "d2")).unwrap().si;
    assert!((report.results["d2"].value - stored).abs() < 1e-12);
}

fn sample_design() -> hm_design::DesignResult {
    design(&DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: Some(parse_quantity("10 m").unwrap()),
        nu: None,
    })
    .unwrap()
}

/// Proyecto completo tal como lo escribe `design` para la duty de ejemplo.
///
/// El cálculo de ejemplo usa 10 m de NPSHa y el proyecto guarda 15 m: es la
/// diferencia con la que ya se escribía el archivo.
fn designed_project() -> Project {
    let result = sample_design();
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.method = METHOD.to_string();
    project.operating.npsha = "15 m".to_string();
    let defaults = ManualDefaults::engine("998.2 kg/m3".to_string(), "0.96".to_string());
    project.params = design_params(&result, None, &defaults).unwrap().0;
    project
}

#[test]
fn override_names_the_parameter_the_reader_uses() {
    let mut project = designed_project();
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
}
