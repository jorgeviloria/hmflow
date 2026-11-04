//! Resultado de un análisis completo.

use hm_core::Diagnostic;
use hm_design::geometry::Geometry;
use hm_design::mechanics::Mechanics;
use hm_design::{CurvePoint, DesignError, DesignInput, DesignResult, ViscousCorrection};

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
