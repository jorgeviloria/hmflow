//! Crear un proyecto y pedir su informe, sin imprimir ni guardar.

use hm_core::PumpType;
use hm_design::meridian::{MeridianEdits, MeridionalModel, WallEdit, meridional_model_with};
use hm_design::plan::{PlanModel, plan_model};
use hm_design::{DesignInput, DesignReport, LEGACY_METHODS, METHOD, design_with_options};
use hm_formula::Registry;
use hm_project::{Casing, Param, Project, Suction};

use crate::analysis::{Analysis, CurvePoints, require_stages};
use crate::audit::{audit_with_analysis, schema_diagnostics};
use crate::case::{Case, DensitySource, Entered, FluidKind, Intake, Temperature, Viscosity};
use crate::convert::{ManualDefaults, case_from_project, casing_fit, design_params};
use crate::dispatch::analyze;
use crate::error::{CaseField, EngineError, Stage, input_error};
use crate::fluid::parse_temperature;
use crate::validate::{design_input, field_code};

/// Crea un proyecto a partir de un caso y un análisis ya calculado.
///
/// Escribe el texto original de cada entrada, `[suction]` si el NPSHa sale de
/// la instalación, el método actual y los parámetros del análisis. `rho` y
/// `eta_m` quedan siempre resueltos. No recalcula: quien llama ya decidió
/// guardar. Exige geometría, mecánica y viscosidad correctas.
pub fn new_project(name: &str, case: &Case, analysis: &Analysis) -> Result<Project, EngineError> {
    if case.design.casing.is_some() {
        return Err(casing_error(
            "este caso lleva carcasa: usa `new_casing_project`, o se perdería",
        ));
    }
    build_project(name, case, analysis, None)
}

/// Como [`new_project`], para un rodete en una carcasa existente. `casing` es
/// el texto escrito: se guarda tal cual, y tiene que dar los valores del caso.
pub fn new_casing_project(
    name: &str,
    case: &Case,
    analysis: &Analysis,
    casing: &Casing,
) -> Result<Project, EngineError> {
    if case.design.casing.as_ref() != Some(&casing_fit(casing)?) {
        return Err(casing_error(
            "el texto de la carcasa no es el del caso analizado",
        ));
    }
    build_project(name, case, analysis, Some(casing))
}

/// Convierte una selección `manual` en decisión: `locked`, con el mismo valor.
/// `recalc --reselect` la conserva. Otro modo no se toca.
pub fn lock_selection(project: &mut Project, name: &str) {
    if let Some(Param::Manual { value }) = project.params.get(name) {
        let value = value.clone();
        project
            .params
            .insert(name.to_string(), Param::Locked { value });
    }
}

fn casing_error(message: &str) -> EngineError {
    input_error(
        CaseField::Param("casing"),
        "HM-PROJ-030",
        format!("[casing] {message}"),
    )
}

fn build_project(
    name: &str,
    case: &Case,
    analysis: &Analysis,
    casing: Option<&Casing>,
) -> Result<Project, EngineError> {
    require_stages(analysis)?;
    check_case_matches_analysis(case, analysis)?;

    let mut project = Project::new(name, case.pump_type);
    project.casing = casing.cloned();
    project.operating.q = case.duty.q.text.clone();
    project.operating.h = case.duty.h.text.clone();
    project.operating.n = case.duty.n.text.clone();
    project.operating.fluid = fluid_name(&case.fluid.kind);
    project.operating.temperature = temperature_text(case.fluid.temperature.as_ref());
    project.operating.viscosity = viscosity_text(case.fluid.viscosity.as_ref());
    project.operating.npsha = npsha_text(&case.intake);
    project.suction = suction_from(&case.intake);
    project.design.method = METHOD.to_string();

    let defaults = ManualDefaults::with_options(
        rho_text(case, analysis),
        analysis.input.eta_m.to_string(),
        &case.design,
    );
    project.params = design_params(&analysis.result, None, &defaults)?.0;

    let validation = project.validate(&Registry::all());
    if validation.has_errors() {
        return Err(EngineError::Invalid {
            diagnostics: validation,
        });
    }
    // Lo que se escribe tiene que poder leerse: el diseño acepta valores que el
    // archivo no, como Z = 101.
    case_from_project(&project)?;
    Ok(project)
}

/// El caso que se escribe tiene que ser el que se analizó, y cada texto tiene
/// que dar su valor.
///
/// `Entered` guarda el texto y la cantidad por separado, y el análisis es un
/// valor aparte. Si no coinciden, el `.hmf` diría una cosa y el cálculo otra,
/// y `check` lo marcaría como `HM-PROJ-015`. Se detecta aquí, al escribir.
pub(crate) fn check_case_matches_analysis(
    case: &Case,
    analysis: &Analysis,
) -> Result<(), EngineError> {
    check_input_matches(case, &analysis.input)?;
    check_texts_match_values(case, analysis)?;

    // Las opciones no se ven en el análisis; sí en lo que dan.
    let from_options = design_with_options(&analysis.input, &case.design)?;
    if format!("{from_options:?}") != format!("{:?}", analysis.result) {
        return Err(input_error(
            CaseField::Param("opciones"),
            field_code(CaseField::Param("opciones")),
            "las opciones del caso no dan el diseño que se analizó",
        ));
    }
    Ok(())
}

/// La entrada que da el caso es la del análisis, campo por campo.
fn check_input_matches(case: &Case, analyzed: &DesignInput) -> Result<(), EngineError> {
    let rebuilt = design_input(case)?;
    let same_option = |a: &Option<hm_units::Quantity>, b: &Option<hm_units::Quantity>| match (a, b)
    {
        (Some(a), Some(b)) => same_value(a.si, b.si),
        (None, None) => true,
        _ => false,
    };
    let checks = [
        (CaseField::Q, same_value(rebuilt.q.si, analyzed.q.si)),
        (CaseField::H, same_value(rebuilt.h.si, analyzed.h.si)),
        (CaseField::N, same_value(rebuilt.n.si, analyzed.n.si)),
        (
            CaseField::Density,
            same_value(rebuilt.rho.si, analyzed.rho.si),
        ),
        (
            CaseField::MechanicalEfficiency,
            same_value(rebuilt.eta_m, analyzed.eta_m),
        ),
        (
            CaseField::Npsha,
            same_option(&rebuilt.npsha, &analyzed.npsha),
        ),
        (
            CaseField::Viscosity,
            match (rebuilt.nu, analyzed.nu) {
                (Some(a), Some(b)) => same_value(a, b),
                (None, None) => true,
                _ => false,
            },
        ),
    ];
    for (field, agrees) in checks {
        if !agrees {
            return Err(input_error(
                field,
                field_code(field),
                "el caso no es el que se analizó",
            ));
        }
    }
    Ok(())
}

/// Cada texto da la cantidad que lleva al lado.
fn check_texts_match_values(case: &Case, analysis: &Analysis) -> Result<(), EngineError> {
    let mut entries: Vec<(CaseField, &Entered)> = vec![
        (CaseField::Q, &case.duty.q),
        (CaseField::H, &case.duty.h),
        (CaseField::N, &case.duty.n),
    ];
    match &case.intake {
        Intake::None => {}
        Intake::Available(npsha) => entries.push((CaseField::Npsha, npsha)),
        Intake::Installation {
            pressure,
            height,
            losses,
            vapor_pressure,
        } => {
            entries.push((CaseField::Suction, pressure));
            for entered in [height, losses, vapor_pressure].into_iter().flatten() {
                entries.push((CaseField::Suction, entered));
            }
        }
    }
    if let Some(Viscosity::Kinematic(entered) | Viscosity::Dynamic(entered)) = &case.fluid.viscosity
    {
        entries.push((CaseField::Viscosity, entered));
    }
    for (field, entered) in entries {
        let agrees = Entered::parse(&entered.text)
            .is_ok_and(|parsed| same_value(parsed.quantity.si, entered.quantity.si));
        if !agrees {
            return Err(disagreement(field, &entered.text));
        }
    }

    if let Some(temperature) = &case.fluid.temperature {
        let agrees = parse_temperature(&temperature.text)
            .is_ok_and(|parsed| same_value(parsed.kelvin, temperature.kelvin));
        if !agrees {
            return Err(disagreement(CaseField::Temperature, &temperature.text));
        }
    }

    if let DensitySource::Explicit { text } = &case.fluid.density.source
        && !text.trim().is_empty()
    {
        let agrees = Entered::parse(text)
            .is_ok_and(|parsed| same_value(parsed.quantity.si, analysis.input.rho.si));
        if !agrees {
            return Err(disagreement(CaseField::Density, text));
        }
    }
    Ok(())
}

pub(crate) fn same_value(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

fn disagreement(field: CaseField, text: &str) -> EngineError {
    input_error(
        field,
        field_code(field),
        format!("el texto «{text}» no da el valor con el que se calculó"),
    )
}

/// Informe de un proyecto end-suction con el método actual.
///
/// El orden es el de hoy: validar, método y tipo, caso y análisis, etapas y
/// solo entonces la auditoría. Los avisos de la auditoría, con los del esquema
/// de parámetros, van al informe y no lo bloquean. No reimplementa el cálculo: es `case_from_project`, `analyze`
/// y `audit_with_analysis`.
pub fn report(project: &Project, curve_points: CurvePoints) -> Result<DesignReport, EngineError> {
    let mut diagnostics = project.validate(&Registry::all());
    if diagnostics.has_errors() {
        return Err(EngineError::Invalid { diagnostics });
    }
    ensure_reportable(project)?;

    let case = case_from_project(project)?;
    let analysis = analyze(&case, curve_points)?;
    let stages = require_stages(&analysis)?;
    diagnostics.extend(schema_diagnostics(project).into_vec());
    diagnostics.extend(audit_with_analysis(project, &analysis).into_vec());

    let mut report = DesignReport::new(
        &analysis.input,
        &analysis.result,
        curve_points.get(),
        diagnostics.into_vec(),
    )
    .with_geometry(stages.geometry)
    .with_mechanics(stages.mechanics);
    if let Some(correction) = stages.viscous {
        report = report.with_viscous(correction);
    }
    Ok(report)
}

/// Vista en planta de un proyecto, la misma que dibuja el visor.
///
/// Rechaza lo mismo que el informe, pero de las etapas solo exige la
/// geometría: es lo único que la planta necesita.
pub fn plan(project: &Project) -> Result<PlanModel, EngineError> {
    let analysis = drawable(project)?;
    let geometry_failed = |source| EngineError::Stage {
        stage: Stage::Geometry,
        source,
    };
    let geometry = analysis
        .geometry
        .as_ref()
        .map_err(|error| geometry_failed(error.clone()))?;
    plan_model(&analysis.result, geometry).map_err(|error| {
        geometry_failed(hm_design::DesignError::Input(format!(
            "vista en planta: {error}"
        )))
    })
}

/// Corte meridional de un proyecto, el mismo que dibuja la pestaña. Rechaza
/// lo mismo que el informe; solo necesita el diseño 1D.
pub fn meridian(project: &Project) -> Result<MeridionalModel, EngineError> {
    let analysis = drawable(project)?;
    let edits = meridian_edits(project);
    meridional_model_with(&analysis.result, &edits).map_err(|source| EngineError::Stage {
        stage: Stage::Geometry,
        source,
    })
}

/// Los brazos de `[meridian]` en mm. Vacío o sin sección es automático. Un
/// texto que no se entienda ya lo rechazó `validate` (HM-PROJ-031).
#[must_use]
pub fn meridian_edits(project: &Project) -> MeridianEdits {
    project
        .meridian
        .as_ref()
        .map(meridian_edits_of)
        .unwrap_or_default()
}

/// Lo editado de una sección `[meridian]`, en mm. Un texto vacío o que no
/// se entiende queda en automático (ya lo marcó `validate`, HM-PROJ-031).
#[must_use]
pub fn meridian_edits_of(section: &hm_project::Meridian) -> MeridianEdits {
    MeridianEdits {
        shroud: wall_edit_of(&section.shroud),
        hub: wall_edit_of(&section.hub),
    }
}

fn wall_edit_of(wall: &hm_project::MeridianWall) -> WallEdit {
    let mm = |text: &str| {
        (!text.trim().is_empty())
            .then(|| hm_units::parse_quantity(text).ok())
            .flatten()
            .filter(|quantity| {
                quantity.dim() == hm_units::Dimension::LENGTH && quantity.si.is_finite()
            })
            .map(|quantity| quantity.si * 1000.0)
    };
    let angle = |text: &str| {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|value| (-90.0..=90.0).contains(value))
    };
    let arm = |text: &str| mm(text).filter(|value| *value > 0.0);
    let p2 = match (mm(&wall.p2_z), mm(&wall.p2_r)) {
        (Some(z), Some(r)) if r > 0.0 => Some([z, r]),
        _ => None,
    };
    WallEdit {
        p2,
        start_angle: angle(&wall.start_angle_deg),
        start_length: arm(&wall.start_length),
        end_angle: angle(&wall.end_angle_deg),
        end_length: arm(&wall.end_length),
    }
}

/// Las comprobaciones del informe y el análisis, para dibujar.
fn drawable(project: &Project) -> Result<Analysis, EngineError> {
    let diagnostics = project.validate(&Registry::all());
    if diagnostics.has_errors() {
        return Err(EngineError::Invalid { diagnostics });
    }
    ensure_reportable(project)?;
    let case = case_from_project(project)?;
    analyze(&case, CurvePoints::MINIMUM)
}

/// Tipo, método anterior y método desconocido. Un método vacío se admite.
fn ensure_reportable(project: &Project) -> Result<(), EngineError> {
    if project.design.pump_type != PumpType::EndSuction {
        return Err(EngineError::Unsupported(project.design.pump_type));
    }
    let method = project.design.method.as_str();
    if LEGACY_METHODS.contains(&method) {
        return Err(EngineError::LegacyMethod {
            method: method.to_string(),
        });
    }
    if !method.is_empty() && method != METHOD {
        return Err(EngineError::UnknownMethod {
            method: method.to_string(),
        });
    }
    Ok(())
}

/// El agua se escribe con su nombre canónico, `water`, sea cual sea la grafía
/// con que se pidió (`Water`, vacío): el proyecto la lee sin distinguir
/// mayúsculas. Otro fluido conserva su nombre.
pub(crate) fn fluid_name(kind: &FluidKind) -> String {
    match kind {
        FluidKind::Water => "water".to_string(),
        FluidKind::Custom(name) => name.clone(),
    }
}

pub(crate) fn temperature_text(temperature: Option<&Temperature>) -> String {
    temperature
        .map(|temperature| temperature.text.clone())
        .unwrap_or_default()
}

pub(crate) fn viscosity_text(viscosity: Option<&Viscosity>) -> String {
    match viscosity {
        Some(Viscosity::Kinematic(entered) | Viscosity::Dynamic(entered)) => entered.text.clone(),
        None => String::new(),
    }
}

/// El NPSHa de la instalación no se copia a `[operating]`: vive en `[suction]`.
pub(crate) fn npsha_text(intake: &Intake) -> String {
    match intake {
        Intake::Available(entered) => entered.text.clone(),
        Intake::None | Intake::Installation { .. } => String::new(),
    }
}

pub(crate) fn suction_from(intake: &Intake) -> Option<Suction> {
    match intake {
        Intake::Installation {
            pressure,
            height,
            losses,
            vapor_pressure,
        } => Some(Suction {
            pressure: pressure.text.clone(),
            height: entered_text(height),
            losses: entered_text(losses),
            vapor_pressure: entered_text(vapor_pressure),
        }),
        Intake::None | Intake::Available(_) => None,
    }
}

fn entered_text(entered: &Option<Entered>) -> String {
    entered
        .as_ref()
        .map(|entered| entered.text.clone())
        .unwrap_or_default()
}

/// El texto explícito se conserva. Si la densidad se derivó, queda con tres
/// decimales, como la CLI cuando no le pasan `--rho`.
pub(crate) fn rho_text(case: &Case, analysis: &Analysis) -> String {
    match &case.fluid.density.source {
        // Un texto vacío no es un `rho` escrito: se trata como derivado.
        DensitySource::Explicit { text } if !text.trim().is_empty() => text.clone(),
        DensitySource::Explicit { .. }
        | DensitySource::WaterAtTemperature
        | DensitySource::WaterAtDefaultTemperature
        | DensitySource::Fallback => format!("{:.3} kg/m3", analysis.input.rho.si),
    }
}
