//! Crear un proyecto y pedir su informe, sin imprimir ni guardar.

use hm_core::PumpType;
use hm_design::{DesignReport, LEGACY_METHODS, METHOD};
use hm_formula::Registry;
use hm_project::{Project, Suction};

use crate::analysis::{Analysis, CurvePoints, require_stages};
use crate::audit::{audit_with_analysis, schema_diagnostics};
use crate::case::{Case, DensitySource, Entered, FluidKind, Intake, Temperature, Viscosity};
use crate::convert::{ManualDefaults, case_from_project, design_params};
use crate::dispatch::analyze;
use crate::error::EngineError;

/// Crea un proyecto a partir de un caso y un análisis ya calculado.
///
/// Escribe el texto original de cada entrada, `[suction]` si el NPSHa sale de
/// la instalación, el método actual y los parámetros del análisis. `rho` y
/// `eta_m` quedan siempre resueltos. No recalcula: quien llama ya decidió
/// guardar. Exige geometría, mecánica y viscosidad correctas.
pub fn new_project(name: &str, case: &Case, analysis: &Analysis) -> Result<Project, EngineError> {
    require_stages(analysis)?;

    let mut project = Project::new(name, case.pump_type);
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
    Ok(project)
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
fn fluid_name(kind: &FluidKind) -> String {
    match kind {
        FluidKind::Water => "water".to_string(),
        FluidKind::Custom(name) => name.clone(),
    }
}

fn temperature_text(temperature: Option<&Temperature>) -> String {
    temperature
        .map(|temperature| temperature.text.clone())
        .unwrap_or_default()
}

fn viscosity_text(viscosity: Option<&Viscosity>) -> String {
    match viscosity {
        Some(Viscosity::Kinematic(entered) | Viscosity::Dynamic(entered)) => entered.text.clone(),
        None => String::new(),
    }
}

/// El NPSHa de la instalación no se copia a `[operating]`: vive en `[suction]`.
fn npsha_text(intake: &Intake) -> String {
    match intake {
        Intake::Available(entered) => entered.text.clone(),
        Intake::None | Intake::Installation { .. } => String::new(),
    }
}

fn suction_from(intake: &Intake) -> Option<Suction> {
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
fn rho_text(case: &Case, analysis: &Analysis) -> String {
    match &case.fluid.density.source {
        // Un texto vacío no es un `rho` escrito: se trata como derivado.
        DensitySource::Explicit { text } if !text.trim().is_empty() => text.clone(),
        DensitySource::Explicit { .. }
        | DensitySource::WaterAtTemperature
        | DensitySource::WaterAtDefaultTemperature
        | DensitySource::Fallback => format!("{:.3} kg/m3", analysis.input.rho.si),
    }
}
