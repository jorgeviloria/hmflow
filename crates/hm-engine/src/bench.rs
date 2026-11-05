//! Un caso de banco reducido frente a la curva de un proyecto.

use hm_bench::{BenchError, Comparison, Reduction, compare};
use hm_project::Project;
use thiserror::Error;

use crate::analysis::CurvePoints;
use crate::convert::case_from_project;
use crate::dispatch::analyze;
use crate::error::EngineError;

/// Puntos de la curva del modelo; la interpolación entre ellos es lineal.
const MODEL_POINTS: usize = 201;

#[derive(Debug, Error)]
pub enum BenchRunError {
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Bench(#[from] BenchError),
}

/// Calcula la curva del proyecto y le compara la reducción.
pub fn compare_with_project(
    reduction: &Reduction,
    project: &Project,
) -> Result<Comparison, BenchRunError> {
    let case = case_from_project(project)?;
    let analysis = analyze(&case, CurvePoints::clamped(MODEL_POINTS))?;
    Ok(compare(reduction, &analysis.curve, analysis.input.n.si)?)
}
