//! Resultado de un análisis completo.

use hm_core::Diagnostic;
use hm_design::geometry::Geometry;
use hm_design::mechanics::Mechanics;
use hm_design::{CurvePoint, DesignError, DesignInput, DesignResult, ViscousCorrection};

use crate::error::{EngineError, Stage};

/// Número de puntos de la curva, comprobado una sola vez.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurvePoints(usize);

impl CurvePoints {
    pub const MIN: usize = 2;
    pub const MAX: usize = 1000;

    /// `None` si `points` cae fuera de 2..=1000.
    pub fn new(points: usize) -> Option<Self> {
        (Self::MIN..=Self::MAX)
            .contains(&points)
            .then_some(Self(points))
    }

    pub fn get(self) -> usize {
        self.0
    }
}

/// Diseño, curva y etapas de un caso.
///
/// `geometry`, `mechanics` y `viscous` guardan el fallo de su etapa. `viscous`
/// es `Ok(None)` cuando no hay viscosidad que aplicar.
#[derive(Debug)]
pub struct Analysis {
    pub input: DesignInput,
    pub result: DesignResult,
    pub curve: Vec<CurvePoint>,
    pub geometry: Result<Geometry, DesignError>,
    pub mechanics: Result<Mechanics, DesignError>,
    pub viscous: Result<Option<ViscousCorrection>, DesignError>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Geometría, mecánica y viscosidad tienen que haber salido.
///
/// `Ok(None)` en la viscosidad es éxito: no había nada que aplicar. El orden
/// es el de la CLI: la primera etapa que falla es la que se devuelve.
pub(crate) fn require_stages(analysis: &Analysis) -> Result<(), EngineError> {
    if let Err(error) = &analysis.geometry {
        return Err(stage_error(Stage::Geometry, error));
    }
    if let Err(error) = &analysis.mechanics {
        return Err(stage_error(Stage::Mechanics, error));
    }
    if let Err(error) = &analysis.viscous {
        return Err(stage_error(Stage::Viscous, error));
    }
    Ok(())
}

fn stage_error(stage: Stage, error: &DesignError) -> EngineError {
    EngineError::Stage {
        stage,
        source: error.clone(),
    }
}
