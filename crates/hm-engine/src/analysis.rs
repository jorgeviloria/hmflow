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

/// Mecánica y viscosidad: lo que exigen `check` y `recalc`. La geometría no.
pub(crate) fn require_mechanics_and_viscosity(analysis: &Analysis) -> Result<(), EngineError> {
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

#[cfg(test)]
mod tests {
    use hm_project::Project;

    use super::*;
    use crate::convert::case_from_project;
    use crate::dispatch::analyze;
    use hm_core::PumpType;

    fn analysis() -> Analysis {
        let project = Project::new("demo", PumpType::EndSuction);
        let case = case_from_project(&project).unwrap();
        analyze(&case, CurvePoints::MINIMUM).unwrap()
    }

    #[test]
    fn check_and_recalc_need_mechanics_and_viscosity_but_not_geometry() {
        let ok = analysis();
        assert!(require_mechanics_and_viscosity(&ok).is_ok());

        let mut no_geometry = analysis();
        no_geometry.geometry = Err(DesignError::Input("sin geometría".into()));
        assert!(require_mechanics_and_viscosity(&no_geometry).is_ok());
        assert!(require_stages(&no_geometry).is_err());

        let mut no_mechanics = analysis();
        no_mechanics.mechanics = Err(DesignError::Input("sin eje".into()));
        assert!(matches!(
            require_mechanics_and_viscosity(&no_mechanics),
            Err(EngineError::Stage {
                stage: Stage::Mechanics,
                ..
            })
        ));

        let mut no_viscosity = analysis();
        no_viscosity.viscous = Err(DesignError::Input("sin ν".into()));
        assert!(matches!(
            require_mechanics_and_viscosity(&no_viscosity),
            Err(EngineError::Stage {
                stage: Stage::Viscous,
                ..
            })
        ));
    }
}
