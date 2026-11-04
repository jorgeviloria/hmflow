//! Despacho del análisis según el tipo de bomba.
//!
//! El `match` de aquí es la costura del segundo tipo. Hoy solo end-suction
//! tiene motor (`end_suction_v0.9`); el resto es `Unsupported`.

use hm_core::{Diagnostics, PumpType};
use hm_design::geometry::geometry;
use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics, mechanics_diagnostics};
use hm_design::{
    DesignError, DesignInput, DesignResult, ViscousCorrection, design_with_options,
    performance_curve,
};

use crate::analysis::{Analysis, CurvePoints};
use crate::case::{Case, Fluid};
use crate::error::EngineError;
use crate::fluid::{fluid_diagnostics, npsh_fluid_diagnostics, viscosity_effect};
use crate::validate::validated_input;

/// Diseño, curva, geometría, mecánica, viscosidad y avisos.
///
/// Falla por la entrada o por el diseño base. Una etapa que no sale se queda
/// en su campo. Los avisos siguen el orden de la CLI: diseño, fluido, NPSH
/// del fluido, viscosidad y mecánica.
pub fn analyze(case: &Case, curve_points: CurvePoints) -> Result<Analysis, EngineError> {
    let input = validated_input(case)?;
    match case.pump_type {
        PumpType::EndSuction => analyze_end_suction(case, input, curve_points),
        other => Err(EngineError::Unsupported(other)),
    }
}

fn analyze_end_suction(
    case: &Case,
    input: DesignInput,
    curve_points: CurvePoints,
) -> Result<Analysis, EngineError> {
    let result = design_with_options(&input, &case.design)?;
    let curve = performance_curve(&input, &result, curve_points.get());
    let geometry = geometry(&input, &result);
    let mechanics_result = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS);
    let (viscous_diagnostics, viscous) = viscous_stage(&case.fluid, &input, &result)?;

    let mut diagnostics = result.diagnostics.clone().into_vec();
    diagnostics.extend(fluid_diagnostics(&case.fluid).into_vec());
    diagnostics.extend(npsh_fluid_diagnostics(&case.fluid, input.npsha.is_some()).into_vec());
    diagnostics.extend(viscous_diagnostics.into_vec());
    if let Ok(mechanics) = &mechanics_result {
        diagnostics.extend(mechanics_diagnostics(mechanics, &result).into_vec());
    }

    Ok(Analysis {
        input,
        result,
        curve,
        geometry,
        mechanics: mechanics_result,
        viscous,
        diagnostics,
    })
}

/// La corrección que no sale es un fallo de etapa, no del análisis.
fn viscous_stage(
    fluid: &Fluid,
    input: &DesignInput,
    result: &DesignResult,
) -> Result<(Diagnostics, Result<Option<ViscousCorrection>, DesignError>), EngineError> {
    match viscosity_effect(fluid, input, result) {
        Ok((diagnostics, correction)) => Ok((diagnostics, Ok(correction))),
        Err(EngineError::Design(error)) => Ok((Diagnostics::new(), Err(error))),
        Err(error) => Err(error),
    }
}
