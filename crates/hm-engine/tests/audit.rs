//! `audit` conserva los diagnósticos que `check` emitía desde la CLI.

use std::collections::BTreeMap;

use hm_core::Diagnostics;
use hm_design::geometry::DEFAULT_INCIDENCE_DEG;
use hm_design::{
    DEFAULT_K_EFF, DEFAULT_VOLUTE_C, DEFAULT_VOLUTE_RATIO, DesignInput, DesignOptions,
    DesignResult, PARAM_SPECS, ParamSpec, design, design_with_options, designed_si,
};
use hm_engine::{
    CurvePoints, METHOD, PumpType, analyze, audit, audit_with_analysis, case_from_project,
};
use hm_project::{Param, Project, Suction};
use hm_units::{Quantity, parse_quantity, parse_unit};

fn codes(diagnostics: &Diagnostics) -> Vec<&str> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn sample_design() -> DesignResult {
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

/// Texto de los parámetros manuales que no salen de la selección del motor.
struct ManualDefaults {
    rho: String,
    eta_m: String,
    eta_v_fixed: String,
    balance_holes: String,
    volute_ratio: String,
    impeller_roughness: String,
    volute_channels: String,
    volute_wrap_deg: String,
    incidence_deg: String,
}

impl ManualDefaults {
    fn engine(rho: String, eta_m: String) -> Self {
        Self::with_options(rho, eta_m, &DesignOptions::default())
    }

    fn with_options(rho: String, eta_m: String, options: &DesignOptions) -> Self {
        Self {
            rho,
            eta_m,
            eta_v_fixed: options.eta_v.unwrap_or(0.0).to_string(),
            balance_holes: u8::from(options.balance_holes).to_string(),
            volute_ratio: options
                .volute_ratio
                .unwrap_or(DEFAULT_VOLUTE_RATIO)
                .to_string(),
            impeller_roughness: format!("{} m", options.impeller_roughness.unwrap_or(0.0)),
            volute_channels: options.volute_channels.unwrap_or(1).to_string(),
            volute_wrap_deg: format!("{} deg", options.volute_wrap_deg.unwrap_or(0.0)),
            incidence_deg: format!(
                "{} deg",
                options.incidence_deg.unwrap_or(DEFAULT_INCIDENCE_DEG)
            ),
        }
    }
}

fn default_manual_text(
    name: &str,
    result: &DesignResult,
    defaults: &ManualDefaults,
) -> Option<String> {
    let selection = &result.selection;
    Some(match name {
        "rho" => defaults.rho.clone(),
        "eta_m" => defaults.eta_m.clone(),
        "mm" => "0.001 m".to_string(),
        "psi" => selection.psi.to_string(),
        "phi" => selection.phi.to_string(),
        "beta2" => format!("{} deg", selection.beta2_deg),
        "blade_count" => selection.blade_count.to_string(),
        "c0" => format!("{} m/s", selection.c0),
        "cm0" => format!("{} m/s", selection.cm0),
        "A" => selection.a.to_string(),
        "J" => selection.j.to_string(),
        "margin" => selection.margin.to_string(),
        "k3" => (result.volute.base_radius.si / (result.d2.si / 2.0)).to_string(),
        "volute_c" => DEFAULT_VOLUTE_C.to_string(),
        "k_eff" => DEFAULT_K_EFF.to_string(),
        "eta_v_fixed" => defaults.eta_v_fixed.clone(),
        "balance_holes" => defaults.balance_holes.clone(),
        "volute_ratio" => defaults.volute_ratio.clone(),
        "impeller_roughness" => defaults.impeller_roughness.clone(),
        "volute_channels" => defaults.volute_channels.clone(),
        "volute_wrap_deg" => defaults.volute_wrap_deg.clone(),
        "incidence_deg" => defaults.incidence_deg.clone(),
        _ => return None,
    })
}

fn auto_text(result: &DesignResult, spec: &ParamSpec) -> String {
    let si = designed_si(result, spec.name)
        .unwrap_or_else(|| panic!("el diseño no calcula `{}`", spec.name));
    if spec.unit == "1" {
        return si.to_string();
    }
    let value = Quantity::from_si(si, parse_unit(spec.unit).unwrap()).value();
    format!("{value} {}", spec.unit)
}

fn manual(value: String) -> Param {
    Param::Manual { value }
}

fn auto(formula: &str, value: String) -> Param {
    Param::Auto {
        formula: formula.to_string(),
        value: Some(value),
    }
}

/// Parámetros del método para `result`, como los escribe `design`.
fn design_params(result: &DesignResult, defaults: &ManualDefaults) -> BTreeMap<String, Param> {
    let mut params = BTreeMap::new();
    for spec in PARAM_SPECS {
        let param = if let Some(formula) = spec.formula {
            auto(formula, auto_text(result, spec))
        } else {
            manual(
                default_manual_text(spec.name, result, defaults)
                    .unwrap_or_else(|| panic!("sin valor por defecto para `{}`", spec.name)),
            )
        };
        params.insert(spec.name.to_string(), param);
    }
    params
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
    project.params = design_params(&result, &defaults);
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
fn fresh_end_suction_clears_the_design_check() {
    let diagnostics = audit(&Project::new("demo", PumpType::EndSuction));
    assert!(
        !diagnostics.has_errors(),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

#[test]
fn other_pump_types_skip_the_end_suction_recompute() {
    let diagnostics = audit(&Project::new("demo", PumpType::Barrel));
    assert_eq!(codes(&diagnostics), ["HM-PROJ-021"]);
    assert!(!diagnostics.has_errors());
}

#[test]
fn end_suction_method_on_barrel_is_reported() {
    let mut project = Project::new("demo", PumpType::Barrel);
    project.design.method = METHOD.to_string();

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-016"));
}

#[test]
fn unknown_method_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.method = "end_suction_v9".to_string();

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-016"));
}

#[test]
fn unknown_only_parameter_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project
        .params
        .insert("d22".to_string(), manual("0.2 m".to_string()));

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-023"));
}

#[test]
fn unknown_parameter_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.method = METHOD.to_string();
    project
        .params
        .insert("d22".to_string(), manual("0.2 m".to_string()));

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-023"));
}

#[test]
fn missing_parameter_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.method = METHOD.to_string();

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-017"));
}

#[test]
fn wrong_formula_is_reported() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.method = METHOD.to_string();
    project
        .params
        .insert("d2".to_string(), auto("design.d1", "0.2 m".to_string()));

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-018"));
}

#[test]
fn override_is_not_audited() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.method = METHOD.to_string();
    project.params.insert(
        "psi".to_string(),
        Param::Override {
            expr: "0.8".to_string(),
            author: "test".to_string(),
            date: "2026-11-01".to_string(),
            reason: "prueba".to_string(),
        },
    );

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-019"));
}

#[test]
fn blade_count_must_be_an_integer() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.method = METHOD.to_string();
    project
        .params
        .insert("blade_count".to_string(), manual("7.5".to_string()));

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-020"));
}

#[test]
fn manual_psi_changes_the_recomputed_design() {
    let designed = sample_design();
    let mut project = Project::new("demo", PumpType::EndSuction);
    project
        .params
        .insert("psi".to_string(), manual("0.5".to_string()));
    project.params.insert(
        "d2".to_string(),
        auto("design.d2", format!("{} m", designed.d2.value())),
    );

    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-015"));
}

#[test]
fn old_manual_slip_points_to_recalc() {
    let mut project = designed_project();
    project
        .params
        .insert("slip".to_string(), manual("0.84".to_string()));
    let diagnostics = audit(&project);
    let hint = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "HM-PROJ-018")
        .and_then(|diagnostic| diagnostic.hint.clone())
        .unwrap_or_default();
    assert!(hint.contains("recalc"), "{hint}");
}

#[test]
fn legacy_method_gets_a_single_migration_error() {
    let mut project = designed_project();
    project.design.method = "end_suction_v0.1".to_string();
    let diagnostics = audit(&project);
    assert_eq!(codes(&diagnostics), ["HM-PROJ-025"]);
}

#[test]
fn project_options_do_not_make_selections_look_stale() {
    // ηv y la razón de la garganta son datos del proyecto, no selecciones.
    let options = DesignOptions {
        eta_v: Some(0.93),
        volute_ratio: Some(0.85),
        ..DesignOptions::default()
    };
    let input = DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: Some(parse_quantity("15 m").unwrap()),
        nu: None,
    };
    let result = design_with_options(&input, &options).unwrap();
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.design.method = METHOD.to_string();
    project.operating.npsha = "15 m".to_string();
    let defaults =
        ManualDefaults::with_options("998.2 kg/m3".to_string(), "0.96".to_string(), &options);
    project.params = design_params(&result, &defaults);
    let diagnostics = audit(&project);
    assert!(
        !codes(&diagnostics).contains(&"HM-DES-013"),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

#[test]
fn project_with_suction_audits_and_reports() {
    let mut project = designed_project();
    project.operating.npsha = String::new();
    project.suction = Some(Suction {
        pressure: "1 atm".to_string(),
        height: "6 m".to_string(),
        losses: String::new(),
        vapor_pressure: String::new(),
    });
    project.operating.viscosity = "20 cSt".to_string();
    // El proyecto se dimensionó con agua. ν entra en discos y anillo, así
    // que los auto guardados ya no cuadran (HM-PROJ-015) hasta un recalc.
    // Lo que no puede pasar es que el anillo deje el diseño sin resultado.
    let diagnostics = audit(&project);
    assert!(
        !codes(&diagnostics).contains(&"HM-PROJ-014"),
        "códigos: {:?}",
        codes(&diagnostics)
    );
    assert!(codes(&diagnostics).contains(&"HM-PROJ-015"));

    let case = case_from_project(&project).unwrap();
    let analysis = analyze(&case, CurvePoints::new(5).unwrap()).unwrap();
    let npsha = analysis.input.npsha.as_ref().expect("hay NPSHa").si;
    assert!(
        (npsha - (101_325.0 - 2339.2) / (998.204 * 9.80665) - 6.0).abs() < 0.01,
        "{npsha}"
    );
    let viscous = analysis
        .viscous
        .expect("la viscosidad no es un fallo de etapa")
        .expect("con viscosidad hay corrección");
    assert!((viscous.nu - 20e-6).abs() < 1e-15);
    assert!(viscous.f_h < 1.0);
}

#[test]
fn dimensionless_mismatch_has_no_unit_suffix() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project
        .params
        .insert("nq".to_string(), auto("similarity.nq", "1".to_string()));
    project
        .params
        .insert("d2".to_string(), auto("design.d2", "9 m".to_string()));
    project
        .params
        .insert("qth".to_string(), auto("design.qth", "1 m3/h".to_string()));

    let diagnostics = audit(&project);
    let nq = mismatch_shown(&diagnostics, "nq");
    assert!(!nq.contains(' '), "adimensional: {nq}");
    let d2 = mismatch_shown(&diagnostics, "d2");
    assert!(d2.ends_with(" m"), "{d2}");
    let qth = mismatch_shown(&diagnostics, "qth");
    assert!(qth.ends_with(" m3/h"), "{qth}");
}

/// Texto entre paréntesis del `HM-PROJ-015` de `name`: número y unidad.
fn mismatch_shown(diagnostics: &Diagnostics, name: &str) -> String {
    let message = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "HM-PROJ-015" && diagnostic.message.contains(name))
        .unwrap_or_else(|| panic!("no hay HM-PROJ-015 de `{name}`"))
        .message
        .as_str();
    let start = message
        .find("recalculado (")
        .unwrap_or_else(|| panic!("sin valor recalculado: {message}"))
        + "recalculado (".len();
    let end = message[start..]
        .find(')')
        .unwrap_or_else(|| panic!("sin cierre: {message}"));
    message[start..start + end].to_string()
}

#[test]
fn a_density_with_the_wrong_dimension_fails_the_check() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project
        .params
        .insert("rho".to_string(), manual("1200 kg".to_string()));
    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-014"));
    assert!(diagnostics.has_errors());
}

#[test]
fn an_efficiency_outside_the_open_interval_fails_the_check() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project
        .params
        .insert("eta_m".to_string(), manual("2".to_string()));
    let diagnostics = audit(&project);
    assert!(codes(&diagnostics).contains(&"HM-PROJ-014"));
}

#[test]
fn a_tampered_auto_value_fails_the_check() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project
        .params
        .insert("d2".to_string(), auto("design.d2", "9 m".to_string()));
    let diagnostics = audit(&project);
    assert!(
        codes(&diagnostics).contains(&"HM-PROJ-015"),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

#[test]
fn a_rounded_auto_value_matches_the_recomputed_design() {
    let project = Project::new("demo", PumpType::EndSuction);
    let diagnostics = audit(&project);
    assert!(
        !diagnostics.has_errors(),
        "códigos: {:?}",
        codes(&diagnostics)
    );

    // El diseño de referencia fija ηm y un NPSHa que el proyecto nuevo no
    // guarda. d2 y dn no se mueven fuera de la tolerancia del `auto`.
    let designed = design(&DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: Some(parse_quantity("10 m").unwrap()),
        nu: None,
    })
    .unwrap();

    let mut project = project;
    project.params.insert(
        "d2".to_string(),
        auto("design.d2", format!("{} m", designed.d2.value())),
    );
    project.params.insert(
        "dn".to_string(),
        auto(
            "design.specific_diameter",
            format!("{:.4}", designed.specific_diameter_mm),
        ),
    );
    let diagnostics = audit(&project);
    assert!(
        !codes(&diagnostics).contains(&"HM-PROJ-015"),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

#[test]
fn a_designed_project_passes_the_check() {
    let diagnostics = audit(&designed_project());
    assert!(
        !diagnostics.has_errors(),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

#[test]
fn slip_and_cu2_are_stored_as_auto() {
    let project = designed_project();
    for name in ["slip", "cu2"] {
        assert!(
            matches!(project.params.get(name), Some(Param::Auto { .. })),
            "`{name}` debe ser auto"
        );
    }
}

#[test]
fn editing_beta2_or_blade_count_is_detected() {
    for (name, value) in [("beta2", "35 deg"), ("blade_count", "4")] {
        let mut project = designed_project();
        project
            .params
            .insert(name.to_string(), manual(value.to_string()));
        let diagnostics = audit(&project);
        assert!(
            codes(&diagnostics).contains(&"HM-PROJ-015"),
            "editar `{name}` no se detectó: {:?}",
            codes(&diagnostics)
        );
    }
}

#[test]
fn selections_outside_their_domain_fail_the_check() {
    for (name, value) in [("margin", "0.5"), ("k_eff", "0"), ("k3", "1")] {
        let mut project = designed_project();
        project
            .params
            .insert(name.to_string(), manual(value.to_string()));
        let diagnostics = audit(&project);
        assert!(
            codes(&diagnostics).contains(&"HM-PROJ-014"),
            "`{name}` = {value} no se rechazó: {:?}",
            codes(&diagnostics)
        );
    }
}

#[test]
fn selections_from_another_duty_are_flagged() {
    let diagnostics = audit(&moved_duty_project());
    assert!(
        codes(&diagnostics).contains(&"HM-DES-013"),
        "códigos: {:?}",
        codes(&diagnostics)
    );
    assert!(!codes(&audit(&designed_project())).contains(&"HM-DES-013"));
}

#[test]
fn locked_selections_are_not_flagged() {
    let mut project = moved_duty_project();
    for name in ["psi", "phi", "beta2", "blade_count", "c0", "cm0", "J"] {
        let value = match &project.params[name] {
            Param::Manual { value } | Param::Locked { value } => value.clone(),
            Param::Auto { value, .. } => value.clone().unwrap_or_default(),
            Param::Override { .. } => String::new(),
        };
        project
            .params
            .insert(name.to_string(), Param::Locked { value });
    }
    let diagnostics = audit(&project);
    assert!(
        !codes(&diagnostics).contains(&"HM-DES-013"),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

/// Guarda de versionado: el `.hmf` de referencia se escribió con
/// `end_suction_v0.9`. Si el motor cambia sus resultados sin cambiar
/// `METHOD`, sus valores `auto` dejan de cuadrar y este test falla. La
/// salida correcta es subir `METHOD`, añadir el anterior a
/// `LEGACY_METHODS` y regenerar el archivo con `design`.
#[test]
fn reference_project_of_the_current_method_audits_clean() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hm-project/tests/fixtures/end_suction_v0.9.hmf");
    let project = Project::load(&path).unwrap();
    assert_eq!(
        project.design.method, METHOD,
        "METHOD cambió: regenera el .hmf de referencia y añade el método anterior a LEGACY_METHODS"
    );
    let diagnostics = audit(&project);
    assert!(
        !diagnostics.has_errors(),
        "códigos: {:?}",
        codes(&diagnostics)
    );
}

#[test]
fn audit_with_analysis_matches_audit_for_the_reference() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../hm-project/tests/fixtures/end_suction_v0.9.hmf");
    let project = Project::load(&path).unwrap();
    let case = case_from_project(&project).unwrap();
    let analysis = analyze(&case, CurvePoints::new(2).unwrap()).unwrap();
    assert_eq!(
        audit(&project).into_vec(),
        audit_with_analysis(&project, &analysis).into_vec()
    );
}
