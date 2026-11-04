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
    /// La curva más corta, para quien solo necesita los diagnósticos.
    pub const MINIMUM: Self = Self(Self::MIN);

    /// `None` si `points` cae fuera de 2..=1000.
    pub fn new(points: usize) -> Option<Self> {
        (Self::MIN..=Self::MAX)
            .contains(&points)
            .then_some(Self(points))
    }

    /// El número de puntos más cercano dentro de 2..=1000.
    pub fn clamped(points: usize) -> Self {
        Self(points.clamp(Self::MIN, Self::MAX))
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

/// Las tres etapas, ya comprobadas.
pub(crate) struct Stages<'a> {
    pub geometry: &'a Geometry,
    pub mechanics: &'a Mechanics,
    /// `None` si no había corrección por viscosidad que aplicar.
    pub viscous: Option<&'a ViscousCorrection>,
}

/// Geometría, mecánica y viscosidad tienen que haber salido.
///
/// `Ok(None)` en la viscosidad es éxito: no había nada que aplicar. El orden
/// es el de la CLI: la primera etapa que falla es la que se devuelve.
pub(crate) fn require_stages(analysis: &Analysis) -> Result<Stages<'_>, EngineError> {
    let geometry = analysis
        .geometry
        .as_ref()
        .map_err(|error| stage_error(Stage::Geometry, error))?;
    let mechanics = analysis
        .mechanics
        .as_ref()
        .map_err(|error| stage_error(Stage::Mechanics, error))?;
    let viscous = analysis
        .viscous
        .as_ref()
        .map_err(|error| stage_error(Stage::Viscous, error))?;
    Ok(Stages {
        geometry,
        mechanics,
        viscous: viscous.as_ref(),
    })
}

fn stage_error(stage: Stage, error: &DesignError) -> EngineError {
    EngineError::Stage {
        stage,
        source: error.clone(),
    }
}
