//! Lo que necesita una ventana que edita el punto de diseño de un proyecto:
//! abrirlo (y decir si sabe editarlo) y reescribir el punto sin tocar el resto.

use hm_core::{Diagnostic, Diagnostics, PumpType};
use hm_design::{LEGACY_METHODS, METHOD};
use hm_formula::Registry;
use hm_project::{Param, Project};
use hm_units::parse_quantity;

use crate::analysis::{Analysis, CurvePoints, require_stages};
use crate::audit::audit;
use crate::case::{Case, DensitySource, FluidKind, Temperature};
use crate::convert::{case_from_project, override_error, param_value};
use crate::dispatch::analyze;
use crate::error::{CaseField, EngineError, cli_message, input_error};
use crate::fluid::{DEFAULT_WATER_TEMPERATURE_K, water_density_at};
use crate::project_ops::{
    check_case_matches_analysis, fluid_name, npsha_text, rho_text, same_value, suction_from,
    temperature_text, viscosity_text,
};
use crate::validate::field_code;

/// Densidad escrita que se admite aunque no sea la del agua a su temperatura.
const RHO_TOLERANCE: f64 = 0.005;

/// Un proyecto que la ventana sabe editar: su caso y lo que la auditoría dice.
#[derive(Debug)]
pub struct Opened {
    pub case: Case,
    pub diagnostics: Diagnostics,
    /// El texto guardado de `eta_m`, para mostrarlo tal cual. La ventana no lee
    /// los parámetros del proyecto.
    pub eta_m_text: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    /// El proyecto tiene errores de formato, un método que no es de este motor
    /// o un parámetro en `override`.
    #[error("el proyecto tiene errores")]
    Invalid { diagnostics: Diagnostics },
    /// Es válido, pero la ventana no lo sabe editar sin perder algo.
    #[error("{reason}")]
    Unsupported { reason: String },
}

/// Abre un proyecto para editarlo. La primera condición que se cumple decide.
///
/// Un `override` en cualquier parámetro lo rechaza: la ventana no lo evalúa y
/// guardar lo perdería de vista. Con el método anterior abre, y la lista de
/// diagnósticos empieza por el aviso `HM-PROJ-025`.
pub fn open_project(project: &Project) -> Result<Opened, EditorError> {
    let validation = project.validate(&Registry::all());
    if validation.has_errors() {
        return Err(EditorError::Invalid {
            diagnostics: validation,
        });
    }
    if project.design.pump_type != PumpType::EndSuction {
        return Err(unsupported(format!(
            "solo hay motor para end-suction; el proyecto es {}",
            project.design.pump_type
        )));
    }
    let method = project.design.method.as_str();
    let legacy = LEGACY_METHODS.contains(&method);
    if !legacy && !method.is_empty() && method != METHOD {
        // `audit` da el `HM-PROJ-016` de siempre.
        return Err(EditorError::Invalid {
            diagnostics: audit(project),
        });
    }
    reject_overrides(project)?;

    let case = case_from_project(project).map_err(invalid_from)?;
    if matches!(case.fluid.kind, FluidKind::Custom(_)) {
        return Err(unsupported("esta interfaz solo edita proyectos de agua"));
    }
    if !project.operating.viscosity.trim().is_empty() {
        return Err(unsupported("esta interfaz no edita una viscosidad escrita"));
    }
    if rho_drifted(&case) {
        return Err(unsupported(
            "esta interfaz solo edita agua con la densidad de su temperatura",
        ));
    }

    let mut diagnostics = audit(project);
    if legacy {
        // El aviso ya va en `audit`; detrás, lo que dice el análisis del caso.
        if let Ok(analysis) = analyze(&case, CurvePoints::MINIMUM) {
            diagnostics.extend(analysis.diagnostics);
        }
    }
    let eta_m_text = project
        .params
        .get("eta_m")
        .and_then(param_value)
        .map(str::to_string);
    Ok(Opened {
        case,
        diagnostics,
        eta_m_text,
    })
}

/// Escribe el punto del caso en un proyecto que ya existe.
///
/// Exige las tres etapas y que el caso sea el que se analizó, como
/// `new_project`; no recalcula ni guarda. Cambian `[operating]`, `[suction]` y,
/// si están ausentes o en `manual`, `rho` y `eta_m`. Los demás parámetros, sus
/// modos, el método y el nombre quedan como estaban: poner al día los `auto` es
/// cosa de `recalc`.
pub fn update_point(
    mut project: Project,
    case: &Case,
    analysis: &Analysis,
) -> Result<Project, EngineError> {
    require_stages(analysis)?;
    check_case_matches_analysis(case, analysis)?;
    for name in ["rho", "eta_m"] {
        if matches!(project.params.get(name), Some(Param::Override { .. })) {
            return Err(override_error(name));
        }
    }
    check_locked_agrees(&project, "rho", analysis.input.rho.si)?;
    check_locked_agrees(&project, "eta_m", analysis.input.eta_m)?;

    project.operating.q = case.duty.q.text.clone();
    project.operating.h = case.duty.h.text.clone();
    project.operating.n = case.duty.n.text.clone();
    project.operating.fluid = fluid_name(&case.fluid.kind);
    project.operating.temperature = temperature_text(case.fluid.temperature.as_ref());
    project.operating.viscosity = viscosity_text(case.fluid.viscosity.as_ref());
    project.operating.npsha = npsha_text(&case.intake);
    project.suction = suction_from(&case.intake);

    rewrite_unless_fixed(&mut project, "rho", rho_text(case, analysis));
    rewrite_unless_fixed(&mut project, "eta_m", analysis.input.eta_m.to_string());

    let validation = project.validate(&Registry::all());
    if validation.has_errors() {
        return Err(EngineError::Invalid {
            diagnostics: validation,
        });
    }
    Ok(project)
}

/// Un `locked` no se toca; por eso tiene que ser el valor con que se calculó.
///
/// Si el usuario cambió algo de lo que depende (la temperatura para `rho`, la
/// eficiencia para `eta_m`), guardar dejaría un archivo que no reproduce lo que
/// vio en pantalla, sin avisar. Se rechaza y se dice qué está bloqueado.
fn check_locked_agrees(
    project: &Project,
    name: &'static str,
    analyzed: f64,
) -> Result<(), EngineError> {
    let Some(Param::Locked { value }) = project.params.get(name) else {
        return Ok(());
    };
    if parse_quantity(value).is_ok_and(|locked| same_value(locked.si, analyzed)) {
        return Ok(());
    }
    Err(input_error(
        CaseField::Param(name),
        field_code(CaseField::Param(name)),
        format!(
            "`{name}` está en locked ({value}) y el cálculo usó otro valor; \
             desbloquéalo en el proyecto o deja el dato como estaba"
        ),
    ))
}

/// Ausente o `manual` se reescribe como `manual`; `locked` se respeta.
fn rewrite_unless_fixed(project: &mut Project, name: &str, value: String) {
    match project.params.get(name) {
        None | Some(Param::Manual { .. }) => {
            project
                .params
                .insert(name.to_string(), Param::Manual { value });
        }
        Some(_) => {}
    }
}

fn unsupported(reason: impl Into<String>) -> EditorError {
    EditorError::Unsupported {
        reason: reason.into(),
    }
}

/// El motor falló al leer un proyecto que ya validaba: es un error del
/// proyecto, con el diagnóstico del motor.
fn invalid_from(error: EngineError) -> EditorError {
    let diagnostic = match error {
        EngineError::Input { diagnostic, .. } => diagnostic,
        other => Diagnostic::error("HM-PROJ-014", cli_message(&other)),
    };
    let mut diagnostics = Diagnostics::new();
    diagnostics.push(diagnostic);
    EditorError::Invalid { diagnostics }
}

/// Cualquier parámetro en `override`, con el mismo diagnóstico que da la
/// auditoría.
fn reject_overrides(project: &Project) -> Result<(), EditorError> {
    let mut diagnostics = Diagnostics::new();
    for (name, param) in &project.params {
        if matches!(param, Param::Override { .. }) {
            diagnostics.push(
                Diagnostic::error(
                    "HM-PROJ-019",
                    format!("`{name}` está en override; la auditoría no evalúa overrides"),
                )
                .with_hint("Usa manual o locked."),
            );
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(EditorError::Invalid { diagnostics })
    }
}

/// Una densidad escrita que se aleja más de 0,5 % de la del agua a su
/// temperatura (20 °C si no hay) ya no es «agua»: guardar la reescribiría.
fn rho_drifted(case: &Case) -> bool {
    let DensitySource::Explicit { text } = &case.fluid.density.source else {
        return false;
    };
    if text.trim().is_empty() {
        return false;
    }
    let default = Temperature {
        text: "20 degC".to_string(),
        kelvin: DEFAULT_WATER_TEMPERATURE_K,
    };
    let temperature = case.fluid.temperature.as_ref().unwrap_or(&default);
    let Some(water) = water_density_at(temperature) else {
        return true;
    };
    (case.fluid.density.value.si - water.si).abs() > RHO_TOLERANCE * water.si
}
