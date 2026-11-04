//! Auditoría de un proyecto: validación, esquema del método y recálculo.
//!
//! `audit` es lo que hace `check`. `audit_with_analysis` parte de un análisis
//! ya hecho y solo añade selecciones y la comparación de los `auto`.

use hm_core::{Diagnostic, Diagnostics, PumpType};
use hm_design::{
    DesignInput, DesignOptions, DesignResult, LEGACY_METHODS, METHOD, PARAM_SPECS,
    design_with_options, designed_si, param_spec, stored_si_matches,
};
use hm_formula::Registry;
use hm_project::{Param, Project};
use hm_units::{Quantity, parse_quantity, parse_unit};

use crate::analysis::{Analysis, CurvePoints};
use crate::case::Case;
use crate::convert::{MAX_COUNT, case_from_project};
use crate::dispatch::analyze;
use crate::error::cli_message;

/// Selecciones que `recalc --reselect` vuelve a tomar del motor para el punto.
///
/// J entra porque el ojo automático lo fija junto con c0 y cm0. Si se deja
/// el J de otro punto, el motor no minimiza el NPSH y c0 queda distinto.
pub(crate) const NQ_SELECTIONS: [&str; 7] =
    ["psi", "phi", "beta2", "blade_count", "c0", "cm0", "J"];

/// Valida el proyecto y, si no hay errores, audita el diseño.
///
/// Con errores de formato no sigue: el recálculo no aportaría un diagnóstico
/// más útil. El orden es el de `check`: validación, o si no diseño, fluido,
/// NPSH del fluido, viscosidad, mecánica, selecciones y `auto`.
pub fn audit(project: &Project) -> Diagnostics {
    let validation = project.validate(&Registry::all());
    if validation.has_errors() {
        return validation;
    }
    design_audit(project)
}

/// Diagnósticos de un análisis ya hecho, más selecciones y `auto` guardados.
///
/// No repite el diseño: `analysis.diagnostics` ya va en el orden canónico.
/// Una etapa fallida se queda en su campo y no se convierte en `HM-PROJ-014`.
pub fn audit_with_analysis(project: &Project, analysis: &Analysis) -> Diagnostics {
    let case = case_from_project(project).ok();
    audit_analysis(project, case.as_ref(), analysis)
}

/// Lo mismo, con el caso que ya se construyó para el análisis.
fn audit_analysis(project: &Project, case: Option<&Case>, analysis: &Analysis) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    diagnostics.extend(analysis.diagnostics.iter().cloned());
    if let Some(case) = case {
        selection_diagnostics(
            project,
            case,
            &analysis.input,
            &analysis.result,
            &mut diagnostics,
        );
    }
    compare_stored_autos(&analysis.result, project, &mut diagnostics);
    diagnostics
}

fn design_audit(project: &Project) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    if LEGACY_METHODS.contains(&project.design.method.as_str()) {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-025",
                format!(
                    "el proyecto se diseñó con {}; el motor actual es {METHOD} y puede dar otros resultados",
                    project.design.method
                ),
            )
            .with_hint(
                "Ejecuta `hmflow-cli recalc --reselect <archivo>` para migrarlo con las \
                 selecciones nuevas, o `recalc` sin --reselect para conservar las tuyas.",
            ),
        );
        return diagnostics;
    }

    if !project.design.method.is_empty() && project.design.method != METHOD {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-016",
                format!("método de diseño desconocido: {}", project.design.method),
            )
            .with_hint(format!("Métodos conocidos: {METHOD}.")),
        );
        return diagnostics;
    }

    if project.design.method == METHOD && project.design.pump_type != PumpType::EndSuction {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-016",
                format!(
                    "el método {METHOD} no aplica a {}",
                    project.design.pump_type
                ),
            )
            .with_hint("Corrige el tipo de bomba o el método de diseño."),
        );
        return diagnostics;
    }

    if project.design.pump_type == PumpType::EndSuction
        && (project.design.method == METHOD || !project.params.is_empty())
    {
        validate_design_schema(project, &mut diagnostics);
    }

    match recompute(project) {
        Ok(Some(design_diagnostics)) => {
            diagnostics.extend(design_diagnostics.into_vec());
        }
        Ok(None) => diagnostics.push(Diagnostic::info(
            "HM-PROJ-021",
            format!(
                "{} todavía no tiene motor de diseño; solo se validó el formato del proyecto",
                project.design.pump_type
            ),
        )),
        // El fallo se resume en HM-PROJ-014, como `check`. El texto de un
        // `Input` entra en el mensaje, sin su código, y no se lista otra vez.
        Err(error) => {
            diagnostics.push(
                Diagnostic::error(
                    "HM-PROJ-014",
                    format!("no se pudo recalcular el diseño: {}", cli_message(&error)),
                )
                .with_hint("Revisa la densidad, la eficiencia mecánica y el punto de operación."),
            );
        }
    }

    diagnostics
}

/// Recalcula un end-suction. `Ok(None)` si el tipo no tiene motor.
///
/// Geometría, mecánica y viscosidad que no salen se quedan en el análisis.
/// Solo la entrada y el diseño base devuelven error, y `audit` lo resume.
fn recompute(project: &Project) -> Result<Option<Diagnostics>, crate::EngineError> {
    if project.design.pump_type != PumpType::EndSuction {
        return Ok(None);
    }

    let case = case_from_project(project)?;
    // Los diagnósticos no dependen de cuántos puntos tenga la curva.
    let points = CurvePoints::new(2).expect("2 está entre 2 y 1000");
    let analysis = analyze(&case, points)?;
    Ok(Some(audit_analysis(project, Some(&case), &analysis)))
}

fn validate_design_schema(project: &Project, diagnostics: &mut Diagnostics) {
    for name in project.params.keys() {
        if param_spec(name).is_none() {
            diagnostics.push(
                Diagnostic::error("HM-PROJ-023", format!("parámetro desconocido: `{name}`"))
                    .with_hint("Revisa el nombre o vuelve a generar el diseño."),
            );
        }
    }

    for spec in PARAM_SPECS {
        let Some(param) = project.params.get(spec.name) else {
            diagnostics.push(
                Diagnostic::error("HM-PROJ-017", format!("falta el parámetro `{}`", spec.name))
                    .with_hint("Vuelve a ejecutar el diseño."),
            );
            continue;
        };

        match (spec.formula, param) {
            (Some(expected), Param::Auto { formula, value }) => {
                if formula != expected {
                    diagnostics.push(Diagnostic::error(
                        "HM-PROJ-018",
                        format!("`{}` usa `{formula}`; se esperaba `{expected}`", spec.name),
                    ));
                }
                match value {
                    Some(value) => check_parameter_unit(diagnostics, spec, value),
                    None => diagnostics.push(Diagnostic::error(
                        "HM-PROJ-017",
                        format!("`{}` no guarda su valor calculado", spec.name),
                    )),
                }
            }
            (Some(_), _) => diagnostics.push(
                Diagnostic::error("HM-PROJ-018", format!("`{}` debe ser auto", spec.name))
                    .with_hint(
                        "Ejecuta `hmflow-cli recalc <archivo>`: lo convierte y calcula su valor.",
                    ),
            ),
            (None, Param::Manual { value } | Param::Locked { value }) => {
                check_parameter_unit(diagnostics, spec, value);
                if spec.name == "blade_count" {
                    check_blade_count(diagnostics, value);
                }
            }
            (None, Param::Override { .. }) => diagnostics.push(
                Diagnostic::error(
                    "HM-PROJ-019",
                    format!(
                        "`{}` está en override; la auditoría no evalúa overrides",
                        spec.name
                    ),
                )
                .with_hint("Usa manual o locked."),
            ),
            (None, Param::Auto { .. }) => diagnostics.push(Diagnostic::error(
                "HM-PROJ-018",
                format!("`{}` debe ser manual o locked", spec.name),
            )),
        }
    }
}

fn check_parameter_unit(diagnostics: &mut Diagnostics, spec: &hm_design::ParamSpec, text: &str) {
    let expected = match parse_unit(spec.unit) {
        Ok(unit) => unit,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                "HM-PROJ-020",
                format!("unidad esperada inválida para `{}`: {error}", spec.name),
            ));
            return;
        }
    };

    match parse_quantity(text) {
        Ok(quantity) => {
            if !quantity.si.is_finite() {
                diagnostics.push(Diagnostic::error(
                    "HM-PROJ-020",
                    format!("`{}` debe ser finito: {text}", spec.name),
                ));
            } else if quantity.dim() != expected.dim {
                diagnostics.push(Diagnostic::error(
                    "HM-PROJ-020",
                    format!("`{}` debe ser {}: {text}", spec.name, spec.unit),
                ));
            }
        }
        Err(error) => diagnostics.push(Diagnostic::error(
            "HM-PROJ-020",
            format!("`{}` no se puede interpretar: {text} ({error})", spec.name),
        )),
    }
}

fn check_blade_count(diagnostics: &mut Diagnostics, text: &str) {
    if let Ok(quantity) = parse_quantity(text)
        && (quantity.si.fract() != 0.0 || !(1.0..=MAX_COUNT).contains(&quantity.si))
    {
        diagnostics.push(Diagnostic::error(
            "HM-PROJ-020",
            format!("`blade_count` debe ser un entero entre 1 y {MAX_COUNT}: {text}"),
        ));
    }
}

fn compare_stored_autos(result: &DesignResult, project: &Project, diagnostics: &mut Diagnostics) {
    for (name, param) in &project.params {
        let Param::Auto {
            value: Some(text), ..
        } = param
        else {
            continue;
        };
        let Some(expected) = designed_si(result, name) else {
            continue;
        };

        match parse_quantity(text) {
            Ok(actual) if stored_si_matches(expected, actual.si) => {}
            Ok(actual) => {
                let shown = Quantity::from_si(expected, actual.unit);
                diagnostics.push(
                    Diagnostic::error(
                        "HM-PROJ-015",
                        format!(
                            "`{name}` guardado como «{text}» no coincide con el diseño recalculado ({:.6}{})",
                            shown.value(),
                            unit_suffix(&shown.unit.symbol)
                        ),
                    )
                    .with_hint(
                        "Ejecuta `hmflow-cli recalc <archivo>` para regenerar los parámetros automáticos.",
                    ),
                );
            }
            Err(error) => {
                diagnostics.push(
                    Diagnostic::error(
                        "HM-PROJ-015",
                        format!(
                            "`{name}` guardado como «{text}» no se puede comparar con el diseño ({error})"
                        ),
                    )
                    .with_hint(
                        "Ejecuta `hmflow-cli recalc <archivo>` para regenerar los parámetros automáticos.",
                    ),
                );
            }
        }
    }
}

/// Unidad para mostrar detrás de un número; vacía si es adimensional.
fn unit_suffix(symbol: &str) -> String {
    if symbol == "1" {
        String::new()
    } else {
        format!(" {symbol}")
    }
}

/// Valor, en SI, de la selección que el motor haría para este punto.
///
/// β2 no se compara: sale de Euler con el Z del proyecto. Si no cuadra con
/// la altura, lo señala `HM-DES-019`.
fn fresh_selection_si(result: &DesignResult, name: &str) -> Option<f64> {
    Some(match name {
        "psi" => result.selection.psi,
        "phi" => result.selection.phi,
        "blade_count" => f64::from(result.selection.blade_count),
        "c0" => result.selection.c0,
        "cm0" => result.selection.cm0,
        "J" => result.selection.j,
        _ => return None,
    })
}

/// Selecciones `manual` que ya no coinciden con lo que el motor elegiría
/// para el punto actual, con el fluido del proyecto y sin otras selecciones
/// manuales. Las `locked` son decisiones explícitas y no se señalan.
fn stale_selections(project: &Project, case: &Case, input: &DesignInput) -> Vec<&'static str> {
    // Sin otras selecciones manuales, pero con los datos del proyecto (ηv,
    // rugosidad, razón de la garganta, voluta…): no son selecciones del motor.
    let data_only = DesignOptions {
        eta_v: case.design.eta_v,
        balance_holes: case.design.balance_holes,
        volute_ratio: case.design.volute_ratio,
        impeller_roughness: case.design.impeller_roughness,
        volute_channels: case.design.volute_channels,
        volute_wrap_deg: case.design.volute_wrap_deg,
        incidence_deg: case.design.incidence_deg,
        ..DesignOptions::default()
    };
    let Ok(fresh) = design_with_options(input, &data_only) else {
        return Vec::new();
    };
    NQ_SELECTIONS
        .into_iter()
        .filter(|name| {
            let Some(Param::Manual { value }) = project.params.get(*name) else {
                return false;
            };
            let (Ok(quantity), Some(expected)) =
                (parse_quantity(value), fresh_selection_si(&fresh, name))
            else {
                return false;
            };
            !stored_si_matches(expected, quantity.si)
        })
        .collect()
}

/// Añade `HM-DES-013` si una selección manual ya no es la del motor.
///
/// No compara los `auto`: eso lo hace `audit`, y meterlo en `recalc`
/// cambiaría su salida.
pub(crate) fn selection_diagnostics(
    project: &Project,
    case: &Case,
    input: &DesignInput,
    result: &DesignResult,
    diagnostics: &mut Diagnostics,
) {
    let stale = stale_selections(project, case, input);
    if stale.is_empty() {
        return;
    }
    let names = stale
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    diagnostics.push(
        Diagnostic::warning(
            "HM-DES-013",
            format!(
                "{names} no {} con la selección del motor para nq = {:.2}; puede venir de otro punto de operación",
                if stale.len() == 1 {
                    "coincide"
                } else {
                    "coinciden"
                },
                result.nq
            ),
        )
        .with_hint(
            "Usa `hmflow-cli recalc --reselect <archivo>` para volver a la selección \
             del motor en este punto, o márcalas como locked si son decisiones tuyas.",
        ),
    );
}
