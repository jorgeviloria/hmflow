//! `analyze` reproduce la secuencia suelta de la CLI y conserva los fallos.

use hm_core::Diagnostic;
use hm_design::geometry::geometry;
use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics, mechanics_diagnostics};
use hm_design::{
    DesignError, DesignInput, DesignOptions, design_with_options, performance_curve, validate_duty,
};
use hm_engine::{
    Analysis, Case, CaseField, CurvePoints, Density, DensitySource, Duty, EngineError, Entered,
    Fluid, FluidKind, Intake, PumpType, Viscosity, analyze, design_input, fluid_diagnostics,
    kinematic_viscosity, npsh_fluid_diagnostics, npsha, parse_temperature, validate_case,
    viscosity_effect,
};
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

/// La secuencia que arma hoy `design` en la CLI, sin imprimir.
struct Loose {
    input: DesignInput,
    result: hm_design::DesignResult,
    curve: Vec<hm_design::CurvePoint>,
    geometry: hm_design::geometry::Geometry,
    mechanics: hm_design::mechanics::Mechanics,
    viscous: Option<hm_design::ViscousCorrection>,
    diagnostics: Vec<Diagnostic>,
}

fn loose(case: &Case, curve_points: usize) -> Loose {
    let npsha = npsha(&case.intake, &case.fluid).unwrap();
    let nu = kinematic_viscosity(&case.fluid).unwrap();
    let eta_m = match case.mechanical_efficiency {
        Some(eta_m) => eta_m,
        None => {
            hm_design::default_mechanical_efficiency(&case.duty.q.quantity, &case.duty.n.quantity)
                .unwrap()
        }
    };
    let input = DesignInput {
        q: case.duty.q.quantity.clone(),
        h: case.duty.h.quantity.clone(),
        n: case.duty.n.quantity.clone(),
        rho: case.fluid.density.value.clone(),
        eta_m,
        npsha,
        nu,
    };
    let result = design_with_options(&input, &case.design).unwrap();
    let curve = performance_curve(&input, &result, curve_points);
    let geometry = geometry(&input, &result).unwrap();
    let mechanics = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let (viscous_diagnostics, viscous) = viscosity_effect(&case.fluid, &input, &result).unwrap();

    let mut diagnostics = result.diagnostics.clone().into_vec();
    diagnostics.extend(fluid_diagnostics(&case.fluid).into_vec());
    diagnostics.extend(npsh_fluid_diagnostics(&case.fluid, input.npsha.is_some()).into_vec());
    diagnostics.extend(viscous_diagnostics.into_vec());
    diagnostics.extend(mechanics_diagnostics(&mechanics, &result).into_vec());

    Loose {
        input,
        result,
        curve,
        geometry,
        mechanics,
        viscous,
        diagnostics,
    }
}

fn same_debug<T: std::fmt::Debug>(actual: &T, expected: &T, what: &str) {
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{what}");
}

fn assert_matches(case: &Case) {
    let analysis = analyze(case, points()).unwrap();
    let loose = loose(case, points().get());
    same_debug(&analysis.input, &loose.input, "entrada");
    same_debug(&analysis.result, &loose.result, "resultado");
    same_debug(&analysis.curve, &loose.curve, "curva");
    same_debug(
        analysis.geometry.as_ref().unwrap(),
        &loose.geometry,
        "geometría",
    );
    same_debug(
        analysis.mechanics.as_ref().unwrap(),
        &loose.mechanics,
        "mecánica",
    );
    same_debug(
        analysis.viscous.as_ref().unwrap(),
        &loose.viscous,
        "viscosidad",
    );
    assert_eq!(analysis.diagnostics, loose.diagnostics);
}

#[test]
fn analyze_matches_the_loose_calls_for_b1_and_d1a1() {
    assert_matches(&b1());
    assert_matches(&d1a1());
}

/// Diseño → fluido → NPSH del fluido → viscosidad → mecánica.
fn canonical_rank(code: &str) -> u8 {
    match code {
        "HM-DES-009" | "HM-DES-010" => 1,
        "HM-DES-018" => 2,
        "HM-DES-020" | "HM-DES-021" | "HM-DES-022" => 3,
        "HM-DES-027" | "HM-DES-028" | "HM-DES-029" | "HM-DES-030" => 4,
        _ => 0,
    }
}

fn assert_canonical(analysis: &Analysis, present: &[u8]) {
    let ranks: Vec<u8> = analysis
        .diagnostics
        .iter()
        .map(|diagnostic| canonical_rank(&diagnostic.code))
        .collect();
    assert!(
        ranks.windows(2).all(|pair| pair[0] <= pair[1]),
        "orden {:?} en {}",
        ranks,
        analysis
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    for rank in present {
        assert!(ranks.contains(rank), "falta el grupo {rank} en {ranks:?}");
    }
}

#[test]
fn diagnostics_follow_the_canonical_order() {
    // nq ≈ 100: la mecánica avisa HM-DES-028. Densidad y viscosidad fuera
    // del agua llenan los grupos de fluido y de viscosidad.
    let mut water = duty_case("954 m3/h", "15 m", "1450 rpm");
    water.fluid.density.value = parse_quantity("1100 kg/m3").unwrap();
    water.fluid.viscosity = Some(Viscosity::Kinematic(entered("100 cSt")));
    let analysis = analyze(&water, points()).unwrap();
    assert_canonical(&analysis, &[0, 1, 3, 4]);
    assert!(analysis.diagnostics.iter().any(|d| d.code == "HM-DES-009"));
    assert!(analysis.diagnostics.iter().any(|d| d.code == "HM-DES-020"));
    assert!(analysis.diagnostics.iter().any(|d| d.code == "HM-DES-028"));

    // Otro fluido, sin viscosidad: NPSH del fluido y el aviso de que no se
    // evaluó. `Ok(None)` no es un fallo.
    let mut custom = duty_case("954 m3/h", "15 m", "1450 rpm");
    custom.fluid.kind = FluidKind::Custom("aceite".to_string());
    custom.fluid.density.value = parse_quantity("900 kg/m3").unwrap();
    custom.intake = Intake::Available(entered("10 m"));
    let analysis = analyze(&custom, points()).unwrap();
    assert_canonical(&analysis, &[0, 2, 3, 4]);
    assert!(matches!(analysis.viscous, Ok(None)));
    assert!(analysis.diagnostics.iter().any(|d| d.code == "HM-DES-018"));
    assert!(analysis.diagnostics.iter().any(|d| d.code == "HM-DES-022"));
    assert!(analysis.diagnostics.iter().any(|d| d.code == "HM-DES-028"));
}

#[test]
fn non_end_suction_is_unsupported() {
    for pump_type in [
        PumpType::Inline,
        PumpType::RingSection,
        PumpType::SplitCase,
        PumpType::Barrel,
    ] {
        let mut case = b1();
        case.pump_type = pump_type;
        let error = analyze(&case, points()).unwrap_err();
        assert!(
            matches!(error, EngineError::Unsupported(found) if found == pump_type),
            "{pump_type}: {error}"
        );
    }
}

#[test]
fn stage_errors_are_preserved() {
    let mut viscous = b1();
    viscous.fluid.viscosity = Some(Viscosity::Kinematic(entered("-1 cSt")));
    let analysis = analyze(&viscous, points()).unwrap();
    assert!(analysis.geometry.is_ok());
    assert!(analysis.mechanics.is_ok());
    assert!(!analysis.curve.is_empty());
    match analysis.viscous {
        Err(DesignError::Input(message)) => {
            assert!(
                message.contains("la viscosidad debe ser positiva"),
                "{message}"
            );
        }
        other => panic!("la viscosidad negativa debía quedar en la etapa: {other:?}"),
    }

    // Caudal grande y razón de garganta baja: el DN tabulado no alcanza.
    // La mecánica y la viscosidad se calculan igual.
    let mut huge = duty_case("4 m3/s", "30 m", "500 rpm");
    huge.design.volute_ratio = Some(0.5);
    let analysis = analyze(&huge, points())
        .unwrap_or_else(|error| panic!("el diseño base no debía fallar: {error}"));
    match &analysis.geometry {
        Err(error) => {
            let message = error.to_string();
            assert!(
                message.contains("tubuladura") || message.contains("difusor"),
                "{message}"
            );
        }
        Ok(_) => panic!("se esperaba un fallo de geometría"),
    }
    assert!(analysis.mechanics.is_ok(), "{:?}", analysis.mechanics);
    assert!(analysis.viscous.is_ok(), "{:?}", analysis.viscous);
    assert_eq!(analysis.curve.len(), points().get());
}

#[test]
fn curve_points_rejects_out_of_range() {
    assert_eq!(CurvePoints::new(0), None);
    assert_eq!(CurvePoints::new(1), None);
    assert_eq!(CurvePoints::new(1001), None);
    assert_eq!(CurvePoints::new(2).unwrap().get(), 2);
    assert_eq!(CurvePoints::new(1000).unwrap().get(), 1000);
}

#[test]
fn curve_points_can_be_clamped_into_range() {
    assert_eq!(CurvePoints::MINIMUM.get(), 2);
    assert_eq!(CurvePoints::clamped(0).get(), 2);
    assert_eq!(CurvePoints::clamped(61).get(), 61);
    assert_eq!(CurvePoints::clamped(5000).get(), 1000);
}

#[test]
fn design_errors_come_back_typed() {
    let mut case = b1();
    case.design.psi = Some(-1.0);
    assert!(validate_case(&case).is_ok());

    let error = analyze(&case, points()).unwrap_err();
    assert_eq!(error.field(), None);
    assert_eq!(error.code(), None);
    match &error {
        EngineError::Design(DesignError::Input(message)) => {
            assert!(message.contains("`psi`"), "{message}");
        }
        other => panic!("se esperaba un error de diseño: {other}"),
    }
}

#[test]
fn duty_with_a_wrong_dimension_is_reported_before_the_efficiency_estimate() {
    let mut case = b1();
    case.mechanical_efficiency = None;
    case.duty.q = entered("3 m");

    let error = design_input(&case).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Q));
    assert_eq!(error.code(), Some("HM-PROJ-002"));
    let message = &error.diagnostic().unwrap().message;
    assert!(message.contains("m3/s"), "{message}");
    assert!(!matches!(error, EngineError::Design(_)));
}

fn assert_field(case: &Case, field: CaseField, code: &str, message: &str) {
    let error = validate_case(case).unwrap_err();
    assert_eq!(error.field(), Some(field), "{error}");
    assert_eq!(error.code(), Some(code));
    assert_eq!(
        error.diagnostic().map(|d| d.message.as_str()),
        Some(message)
    );
}

#[test]
fn validate_case_names_the_failing_field() {
    let mut case = b1();
    case.duty.q = entered("-1 m3/s");
    let duty = validate_duty(
        &case.duty.q.quantity,
        &case.duty.h.quantity,
        &case.duty.n.quantity,
    )
    .unwrap_err();
    assert_eq!(duty.name, "Q");
    assert_field(&case, CaseField::Q, "HM-PROJ-002", &duty.message);

    let mut case = b1();
    case.duty.h = entered("-1 m");
    let duty = validate_duty(
        &case.duty.q.quantity,
        &case.duty.h.quantity,
        &case.duty.n.quantity,
    )
    .unwrap_err();
    assert_eq!(duty.name, "H");
    assert_field(&case, CaseField::H, "HM-PROJ-003", &duty.message);

    let mut case = b1();
    case.duty.n = entered("-1 rpm");
    let duty = validate_duty(
        &case.duty.q.quantity,
        &case.duty.h.quantity,
        &case.duty.n.quantity,
    )
    .unwrap_err();
    assert_eq!(duty.name, "n");
    assert_field(&case, CaseField::N, "HM-PROJ-004", &duty.message);

    let mut case = b1();
    case.intake = Intake::Available(entered("2 s"));
    let input = design_input(&case).unwrap();
    let field = input.validate().unwrap_err();
    assert_eq!(field.name, "NPSHa");
    assert_field(&case, CaseField::Npsha, "HM-PROJ-005", &field.message);

    let mut case = b1();
    case.fluid.density.value = parse_quantity("1 m").unwrap();
    let input = design_input(&case).unwrap();
    let field = input.validate().unwrap_err();
    assert_eq!(field.name, "rho");
    assert_field(&case, CaseField::Density, "HM-PROJ-020", &field.message);

    let mut case = b1();
    case.mechanical_efficiency = Some(1.0);
    let input = design_input(&case).unwrap();
    let field = input.validate().unwrap_err();
    assert_eq!(field.name, "eta_m");
    assert_field(
        &case,
        CaseField::MechanicalEfficiency,
        "HM-PROJ-020",
        &field.message,
    );

    let mut case = b1();
    case.design.eta_v = Some(1.0);
    let field = case.design.validate().unwrap_err();
    assert_eq!(field.name, "eta_v_fixed");
    assert_field(
        &case,
        CaseField::Param("eta_v_fixed"),
        "HM-PROJ-020",
        &field.message,
    );
}

#[test]
fn out_of_range_water_temperature_is_not_a_case_error() {
    let mut case = b1();
    case.fluid.temperature = Some(parse_temperature("200 degC").unwrap());
    assert!(validate_case(&case).is_ok());

    let analysis = analyze(&case, points()).unwrap();
    assert!(analysis.diagnostics.iter().any(|d| d.code == "HM-DES-010"));
    assert!(matches!(analysis.viscous, Ok(None)));
}
