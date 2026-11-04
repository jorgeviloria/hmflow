//! Errores del motor de aplicación.

use hm_core::{Diagnostic, Diagnostics, PumpType};

/// Frase de la CLI cuando la temperatura cae fuera de IAPWS.
pub(crate) const VAPOR_PRESSURE_RANGE: &str =
    "la temperatura está fuera del intervalo de la presión de vapor del agua";

pub(crate) fn input_error(
    field: CaseField,
    code: &'static str,
    message: impl Into<String>,
) -> EngineError {
    EngineError::Input {
        field,
        diagnostic: Diagnostic::error(code, message),
    }
}

/// Texto que ya mostraba la CLI. Un `Input` no antepone su código.
pub fn cli_message(error: &EngineError) -> String {
    match error {
        EngineError::Input { diagnostic, .. } => diagnostic.message.clone(),
        other => other.to_string(),
    }
}

/// Campo del caso al que pertenece un error de entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseField {
    Q,
    H,
    N,
    Temperature,
    Density,
    MechanicalEfficiency,
    Viscosity,
    Npsha,
    Suction,
    Param(&'static str),
}

/// Etapa opcional que puede fallar sin impedir el resto del análisis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Geometry,
    Mechanics,
    Viscous,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{}: {}", diagnostic.code, diagnostic.message)]
    Input {
        field: CaseField,
        diagnostic: Diagnostic,
    },

    #[error(transparent)]
    Design(#[from] hm_design::DesignError),

    #[error(transparent)]
    Project(#[from] hm_project::ProjectError),

    #[error("el proyecto es de {method}; migra con `hmflow-cli recalc` antes de continuar")]
    LegacyMethod { method: String },

    #[error("método de diseño desconocido: {method}")]
    UnknownMethod { method: String },

    #[error("solo hay motor para end-suction; el proyecto es {0}")]
    Unsupported(PumpType),

    #[error("el proyecto o el diseño tienen errores")]
    Invalid { diagnostics: Diagnostics },

    #[error("falló la etapa {stage:?}: {source}")]
    Stage {
        stage: Stage,
        source: hm_design::DesignError,
    },
}

impl EngineError {
    pub fn field(&self) -> Option<CaseField> {
        match self {
            Self::Input { field, .. } => Some(*field),
            Self::Design(_)
            | Self::Project(_)
            | Self::LegacyMethod { .. }
            | Self::UnknownMethod { .. }
            | Self::Unsupported(_)
            | Self::Invalid { .. }
            | Self::Stage { .. } => None,
        }
    }

    pub fn diagnostic(&self) -> Option<&Diagnostic> {
        match self {
            Self::Input { diagnostic, .. } => Some(diagnostic),
            Self::Design(_)
            | Self::Project(_)
            | Self::LegacyMethod { .. }
            | Self::UnknownMethod { .. }
            | Self::Unsupported(_)
            | Self::Invalid { .. }
            | Self::Stage { .. } => None,
        }
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Input { diagnostic, .. } => Some(diagnostic.code.as_str()),
            Self::LegacyMethod { .. } => Some("HM-PROJ-025"),
            Self::UnknownMethod { .. } => Some("HM-PROJ-016"),
            Self::Design(_)
            | Self::Project(_)
            | Self::Unsupported(_)
            | Self::Invalid { .. }
            | Self::Stage { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hm_core::Diagnostic;

    #[test]
    fn input_errors_know_their_field_and_code() {
        let error = EngineError::Input {
            field: CaseField::Q,
            diagnostic: Diagnostic::error("HM-PROJ-002", "caudal inválido"),
        };

        assert_eq!(error.field(), Some(CaseField::Q));
        assert_eq!(error.code(), Some("HM-PROJ-002"));
        assert_eq!(
            error
                .diagnostic()
                .map(|diagnostic| diagnostic.code.as_str()),
            Some("HM-PROJ-002")
        );
    }

    #[test]
    fn other_errors_have_no_field() {
        let error = EngineError::Unsupported(PumpType::Inline);

        assert_eq!(error.field(), None);
        assert_eq!(error.diagnostic(), None);
        assert_eq!(error.code(), None);
    }

    #[test]
    fn legacy_and_unknown_methods_have_codes() {
        let legacy = EngineError::LegacyMethod {
            method: "end_suction_v0.8".to_string(),
        };
        let unknown = EngineError::UnknownMethod {
            method: "end_suction_v9".to_string(),
        };

        assert_eq!(legacy.code(), Some("HM-PROJ-025"));
        assert_eq!(unknown.code(), Some("HM-PROJ-016"));
    }
}
