//! Recálculo de un proyecto end-suction: migra el método y reescribe los `auto`.

use hm_core::{Diagnostics, PumpType};
use hm_design::{LEGACY_METHODS, METHOD, param_spec};
use hm_formula::Registry;
use hm_project::{Param, Project};
use hm_units::parse_quantity;

use crate::analysis::{Analysis, CurvePoints, require_mechanics_and_viscosity};
use crate::audit::{NQ_SELECTIONS, selection_diagnostics};
use crate::case::Case;
use crate::changes::{ChangeKind, ParamChange};
use crate::convert::{
    ManualDefaults, case_from_project, casing_results, casing_volute, design_params, eye_is_fixed,
    is_eye_selection, param_value,
};
use crate::dispatch::analyze;
use crate::error::EngineError;

/// Proyecto reescrito, con los cambios y los diagnósticos del diseño.
///
/// Los diagnósticos siguen el orden de la CLI: diseño, fluido, NPSH del
/// fluido, viscosidad, mecánica y selecciones. No incluyen la comparación
/// de los `auto`, porque `recalc` los regenera.
#[derive(Debug)]
pub struct Recalculated {
    pub project: Project,
    pub changes: Vec<ParamChange>,
    pub diagnostics: Diagnostics,
}

/// Recalcula un proyecto end-suction y devuelve el proyecto reescrito.
///
/// Valida, migra un método anterior y reescribe los `auto`. Conserva los
/// manuales y los bloqueados. Con `reselect`, las selecciones manuales que
/// dependen del punto vuelven a salir de la tabla del motor. No imprime ni
/// guarda.
pub fn recalc(mut project: Project, reselect: bool) -> Result<Recalculated, EngineError> {
    let validation = project.validate(&Registry::all());
    if validation.has_errors() {
        return Err(EngineError::Invalid {
            diagnostics: validation,
        });
    }
    if project.design.pump_type != PumpType::EndSuction {
        return Err(EngineError::Unsupported(project.design.pump_type));
    }
    let migrated_from = migrated_method(&project.design.method)?;
    if let Some(name) = project
        .params
        .keys()
        .find(|name| param_spec(name).is_none())
    {
        return Err(unknown_param(name));
    }

    // Hasta end_suction_v0.3, ηm incluía el rozamiento de discos; desde la
    // v0.4 va aparte, así que el ηm guardado ya no significa lo mismo.
    let lumped_eta_m = matches!(
        migrated_from.as_deref(),
        Some("end_suction_v0.1" | "end_suction_v0.2" | "end_suction_v0.3")
    );
    let old_eta_m = if lumped_eta_m {
        let removed = project.params.remove("eta_m");
        removed.as_ref().and_then(param_value).map(str::to_string)
    } else {
        None
    };
    let mut reselected = if reselect {
        take_manual_selections(&mut project)
    } else {
        Vec::new()
    };
    // Se rehacen con el punto actual: el archivo no debe conservar unos viejos.
    let copied: Vec<_> = casing_results(&project)
        .into_iter()
        .chain(casing_volute(&project))
        .collect();
    for name in copied {
        if matches!(project.params.get(name), Some(Param::Manual { .. }))
            && let Some(Param::Manual { value }) = project.params.remove(name)
        {
            reselected.push((name, value));
        }
    }

    let case = case_from_project(&project)?;
    let analysis = analyze(&case, CurvePoints::MINIMUM)?;
    require_mechanics_and_viscosity(&analysis)?;

    let diagnostics = recalc_diagnostics(&project, &case, &analysis);
    if diagnostics.has_errors() {
        return Err(EngineError::DesignHasErrors { diagnostics });
    }

    let defaults = ManualDefaults::engine(
        format!("{} kg/m3", analysis.input.rho.si),
        analysis.input.eta_m.to_string(),
    );
    let (params, filled) = design_params(&analysis.result, Some(&project.params), &defaults)
        .map_err(|error| after_design(&diagnostics, error))?;
    project.params = params;
    project.design.method = METHOD.to_string();

    let validation = project.validate(&Registry::all());
    if validation.has_errors() {
        return Err(after_design(
            &diagnostics,
            EngineError::RecalculatedInvalid {
                diagnostics: validation,
            },
        ));
    }

    let changes = assemble_changes(
        &project,
        &reselected,
        &filled,
        old_eta_m,
        analysis.input.eta_m,
        migrated_from,
    );
    Ok(Recalculated {
        project,
        changes,
        diagnostics,
    })
}

/// Un fallo posterior al diseño, con los avisos que el diseño ya dio.
fn after_design(design: &Diagnostics, source: EngineError) -> EngineError {
    EngineError::AfterDesign {
        design: design.clone(),
        source: Box::new(source),
    }
}

/// `Some` si el método es anterior y se puede migrar.
fn migrated_method(method: &str) -> Result<Option<String>, EngineError> {
    if method.is_empty() || method == METHOD {
        return Ok(None);
    }
    if LEGACY_METHODS.contains(&method) {
        return Ok(Some(method.to_string()));
    }
    Err(EngineError::UnknownMethod {
        method: method.to_string(),
    })
}

fn unknown_param(name: &str) -> EngineError {
    EngineError::UnknownParameter {
        name: name.to_string(),
    }
}

/// Quita las selecciones manuales del punto y devuelve su texto anterior.
pub(crate) fn take_manual_selections(project: &mut Project) -> Vec<(&'static str, String)> {
    let mut reselected = Vec::new();
    let eye_fixed = eye_is_fixed(project);
    for name in NQ_SELECTIONS {
        if eye_fixed && is_eye_selection(name) {
            continue;
        }
        if let Some(Param::Manual { value }) = project.params.get(name) {
            reselected.push((name, value.clone()));
        }
    }
    for (name, _) in &reselected {
        project.params.remove(*name);
    }
    reselected
}

fn recalc_diagnostics(project: &Project, case: &Case, analysis: &Analysis) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    diagnostics.extend(analysis.diagnostics.iter().cloned());
    selection_diagnostics(
        project,
        case,
        &analysis.input,
        &analysis.result,
        &mut diagnostics,
    );
    diagnostics
}

fn assemble_changes(
    project: &Project,
    reselected: &[(&'static str, String)],
    filled: &[String],
    old_eta_m: Option<String>,
    eta_m: f64,
    migrated_from: Option<String>,
) -> Vec<ParamChange> {
    let mut changes = Vec::new();
    for (name, old) in reselected {
        let value = stored_text(project, name);
        if !same_value(old, &value) {
            changes.push(ParamChange {
                name: (*name).to_string(),
                before: Some(old.clone()),
                after: value,
                kind: ChangeKind::Reselected,
            });
        }
    }
    for name in filled {
        let replaced = reselected.iter().any(|(old, _)| *old == name);
        if replaced || (old_eta_m.is_some() && name == "eta_m") {
            continue;
        }
        changes.push(ParamChange {
            name: name.clone(),
            before: None,
            after: stored_text(project, name),
            kind: ChangeKind::Filled,
        });
    }
    if let Some(old) = old_eta_m {
        changes.push(ParamChange {
            name: "eta_m".to_string(),
            before: Some(old),
            after: eta_m.to_string(),
            kind: ChangeKind::MechanicalEfficiency,
        });
    }
    if let Some(old) = migrated_from {
        changes.push(ParamChange {
            name: "method".to_string(),
            before: Some(old),
            after: METHOD.to_string(),
            kind: ChangeKind::MethodMigrated,
        });
    }
    changes
}

/// Con tolerancia: el redondeo de `rho` en el archivo mueve la duodécima cifra de
/// lo que depende de ν.
fn same_value(old: &str, new: &str) -> bool {
    match (parse_quantity(old), parse_quantity(new)) {
        (Ok(old), Ok(new)) => crate::project_ops::same_value(old.si, new.si),
        _ => old == new,
    }
}

fn stored_text(project: &Project, name: &str) -> String {
    project
        .params
        .get(name)
        .and_then(param_value)
        .unwrap_or("")
        .to_string()
}
