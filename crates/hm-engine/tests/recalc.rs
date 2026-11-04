//! `recalc` migra, rellena y reescribe un proyecto sin imprimir ni guardar.

use hm_core::Diagnostics;
use hm_design::{DesignInput, default_mechanical_efficiency, design};
use hm_engine::{
    ChangeKind, EngineError, METHOD, ManualDefaults, PumpType, audit, case_from_project,
    cli_message, design_input, design_params, recalc,
};
use hm_project::{Param, Project};
use hm_units::parse_quantity;

fn codes(diagnostics: &Diagnostics) -> Vec<&str> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
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

fn manual(value: &str) -> Param {
    Param::Manual {
        value: value.to_string(),
    }
}

fn param_text<'a>(project: &'a Project, name: &str) -> Option<&'a str> {
    match project.params.get(name)? {
        Param::Manual { value } | Param::Locked { value } => Some(value),
        Param::Auto { value, .. } => value.as_deref(),
        Param::Override { .. } => None,
    }
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

/// Proyecto diseñado para 120 m3/h cuyo caudal pasa a 250 m3/h: nq sube
/// de 30,5 a 44 y cambia de tramo en la tabla.
fn moved_duty_project() -> Project {
    let mut project = designed_project();
    project.operating.q = "250 m3/h".to_string();
    project.operating.npsha = "25 m".to_string();
    project
}

#[test]
fn recalc_migrates_a_v08_project() {
    // El .hmf de referencia de la v0.8, con A y J manuales. `recalc`
    // lo lleva a la v0.9 y conserva las selecciones manuales.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hm-project/tests/fixtures/end_suction_v0.8.hmf");
    let before = Project::load(&fixture).unwrap();
    assert_eq!(before.design.method, "end_suction_v0.8");

    let done = recalc(before.clone(), false).unwrap();

    assert_eq!(done.project.design.method, "end_suction_v0.9");
    assert_eq!(
        format!("{:?}", done.project.params["J"]),
        format!("{:?}", before.params["J"])
    );
    let diagnostics = audit(&done.project);
    assert!(
        !diagnostics.has_errors(),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

#[test]
fn recalc_migrates_a_legacy_project() {
    let mut project = designed_project();
    project.design.method = "end_suction_v0.3".to_string();
    // Hasta la v0.3, ηm incluía el rozamiento de discos.
    project.params.insert("eta_m".to_string(), manual("0.96"));

    let done = recalc(project, true).unwrap();

    assert_eq!(done.project.design.method, METHOD);
    let case = case_from_project(&done.project).unwrap();
    let input = design_input(&case).unwrap();
    let expected = default_mechanical_efficiency(&input.q, &input.n).unwrap();
    assert!(
        (input.eta_m - expected).abs() < 1e-12,
        "eta_m {} frente a la estimación {expected}",
        input.eta_m
    );
    let diagnostics = audit(&done.project);
    assert!(
        !diagnostics.has_errors(),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

#[test]
fn recalc_from_v04_keeps_the_mechanical_efficiency() {
    // En la v0.4 ηm ya no incluía los discos: migrar no lo toca.
    let mut project = designed_project();
    project.design.method = "end_suction_v0.4".to_string();
    project.params.insert("eta_m".to_string(), manual("0.975"));

    let done = recalc(project, false).unwrap();

    assert_eq!(done.project.design.method, METHOD);
    let case = case_from_project(&done.project).unwrap();
    let input = design_input(&case).unwrap();
    assert!((input.eta_m - 0.975).abs() < 1e-12, "eta_m {}", input.eta_m);
}

#[test]
fn recalc_keeps_manual_edits_and_clears_the_check() {
    let mut project = designed_project();
    project.params.insert("beta2".to_string(), manual("30 deg"));
    assert!(audit(&project).has_errors());

    let done = recalc(project, false).unwrap();
    let diagnostics = audit(&done.project);
    assert!(
        !diagnostics.has_errors(),
        "códigos: {:?}",
        codes(&diagnostics)
    );
    assert_eq!(param_text(&done.project, "beta2"), Some("30 deg"));
}

#[test]
fn recalc_fills_missing_manual_parameters() {
    let mut project = designed_project();
    project.params.remove("k_eff");

    let done = recalc(project, false).unwrap();
    let filled: Vec<_> = done
        .changes
        .iter()
        .filter(|change| change.kind == ChangeKind::Filled)
        .map(|change| change.name.as_str())
        .collect();
    assert_eq!(filled, ["k_eff"]);
    assert!(done.project.params.contains_key("k_eff"));
}

#[test]
fn recalc_does_not_replace_an_override() {
    let mut project = designed_project();
    project.params.insert(
        "mm".to_string(),
        Param::Override {
            expr: "0.001".to_string(),
            author: "test".to_string(),
            date: "2026-11-02".to_string(),
            reason: "prueba".to_string(),
        },
    );
    let result = sample_design();
    let defaults = ManualDefaults::engine("998.2 kg/m3".to_string(), "0.96".to_string());
    let error = design_params(&result, Some(&project.params), &defaults).unwrap_err();
    assert_eq!(
        cli_message(&error),
        "`mm` debe ser manual o locked para recalcular"
    );
    assert!(recalc(project, false).is_err());
}

#[test]
fn reselect_takes_the_table_for_the_new_nq() {
    let mut project = moved_duty_project();
    project.params.insert(
        "blade_count".to_string(),
        Param::Locked {
            value: "7".to_string(),
        },
    );

    let done = recalc(project, true).unwrap();

    // ψ de Gülich (3.26) para nq = 44.
    let psi: f64 = param_text(&done.project, "psi").unwrap().parse().unwrap();
    assert!(
        (psi - 1.21 * (-0.77f64 * 0.4398).exp()).abs() < 1e-3,
        "ψ = {psi}"
    );
    assert_eq!(
        param_text(&done.project, "blade_count"),
        Some("7"),
        "una selección locked no se toca"
    );
    let diagnostics = audit(&done.project);
    assert!(
        !diagnostics.has_errors(),
        "códigos: {:?}",
        codes(&diagnostics)
    );
    assert!(!codes(&diagnostics).contains(&"HM-DES-013"));
}

#[test]
fn recalc_without_reselect_keeps_the_old_selections() {
    let before = moved_duty_project();
    let psi_before = param_text(&before, "psi").unwrap().to_string();

    let done = recalc(before, false).unwrap();

    assert_eq!(param_text(&done.project, "psi"), Some(psi_before.as_str()));
    assert!(codes(&audit(&done.project)).contains(&"HM-DES-013"));
}

fn error_codes(error: &EngineError) -> Vec<String> {
    match error {
        EngineError::Invalid { diagnostics } | EngineError::DesignHasErrors { diagnostics } => {
            diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect()
        }
        other => panic!("se esperaba Invalid, llegó {other:?}"),
    }
}

#[test]
fn a_project_with_format_errors_is_not_recalculated() {
    let mut project = designed_project();
    project.operating.q = "mucho".to_string();

    let error = recalc(project, false).unwrap_err();
    assert!(error_codes(&error).contains(&"HM-PROJ-002".to_string()));
}

#[test]
fn only_end_suction_is_recalculated() {
    let mut project = Project::new("demo", PumpType::Inline);
    project.design.method = METHOD.to_string();

    let error = recalc(project, false).unwrap_err();
    assert!(
        matches!(error, EngineError::Unsupported(PumpType::Inline)),
        "{error:?}"
    );
}

#[test]
fn an_unknown_method_is_rejected_with_its_name() {
    let mut project = designed_project();
    project.design.method = "end_suction_v9".to_string();

    match recalc(project, false).unwrap_err() {
        EngineError::UnknownMethod { method } => assert_eq!(method, "end_suction_v9"),
        // La validación del proyecto también conoce los métodos.
        error @ EngineError::Invalid { .. } => {
            assert!(error_codes(&error).contains(&"HM-PROJ-016".to_string()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unknown_parameter_stops_the_recalculation() {
    let mut project = designed_project();
    project.params.insert("inventado".to_string(), manual("1"));

    match recalc(project, false).unwrap_err() {
        EngineError::UnknownParameter { name } => assert_eq!(name, "inventado"),
        // La validación del proyecto también conoce los parámetros.
        error @ EngineError::Invalid { .. } => {
            assert!(error_codes(&error).contains(&"HM-PROJ-023".to_string()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_design_with_errors_is_not_written() {
    let mut project = designed_project();
    project.operating.npsha = "0.1 m".to_string();

    let error = recalc(project, false).unwrap_err();
    assert!(
        matches!(error, EngineError::DesignHasErrors { .. }),
        "{error:?}"
    );
    assert!(error_codes(&error).contains(&"HM-DES-007".to_string()));
}

#[test]
fn a_geometry_that_does_not_come_out_does_not_stop_the_recalculation() {
    // Caudal enorme y garganta baja: el DN tabulado no alcanza y la geometría
    // no sale. `recalc` no usa la geometría, así que, como antes, recalcula.
    let mut project = designed_project();
    project.operating.q = "14400 m3/h".to_string();
    project.operating.h = "30 m".to_string();
    project.operating.n = "500 rpm".to_string();
    project.operating.npsha = String::new();
    project
        .params
        .insert("volute_ratio".to_string(), manual("0.5"));

    // Con `reselect`, las selecciones del punto viejo vuelven a salir del motor.
    let done = recalc(project, true).expect("la geometría no bloquea el recálculo");
    assert_eq!(done.project.design.method, METHOD);
}
