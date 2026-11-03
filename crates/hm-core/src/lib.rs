//! Tipos transversales: diagnósticos, severidad y revisión.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

use serde::{Deserialize, Serialize};

pub mod codes;

pub use codes::{CODES, CodeInfo, CodeStatus, code_info};

/// Gravedad de un diagnóstico.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Blocking,
}

/// Un mensaje con código estable, origen y sugerencia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Diagnostic {
    /// Crea un diagnóstico. En compilación de depuración comprueba que el
    /// código está en [`CODES`], activo y con la misma severidad.
    pub fn new(severity: Severity, code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        debug_assert!(
            code_info(&code)
                .is_some_and(|info| info.status == CodeStatus::Active && info.severity == severity),
            "código {code} no registrado, retirado o con otra severidad en hm_core::codes"
        );
        Self {
            severity,
            code,
            message: message.into(),
            hint: None,
        }
    }

    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, code, message)
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    pub fn blocking(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Blocking, code, message)
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error | Severity::Blocking)
    }
}

/// Número monotónico que invalida cachés.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Revision(pub u64);

impl Revision {
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Colección ordenada de diagnósticos.
#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(other);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(Diagnostic::is_error)
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_detected() {
        let mut d = Diagnostics::new();
        d.push(Diagnostic::warning("HM-DES-001", "aviso"));
        assert!(!d.has_errors());
        d.push(Diagnostic::error("HM-PROJ-007", "error"));
        assert!(d.has_errors());
        assert_eq!(d.len(), 2);
    }

    #[test]
    #[should_panic(expected = "no registrado")]
    #[cfg(debug_assertions)]
    fn unregistered_codes_are_rejected_in_debug() {
        let _ = Diagnostic::warning("HM-XXX-999", "inventado");
    }

    #[test]
    #[should_panic(expected = "otra severidad")]
    #[cfg(debug_assertions)]
    fn severity_must_match_the_registry() {
        let _ = Diagnostic::error("HM-DES-001", "debería ser aviso");
    }

    #[test]
    fn revision_advances() {
        assert_eq!(Revision(1).next(), Revision(2));
    }
}
