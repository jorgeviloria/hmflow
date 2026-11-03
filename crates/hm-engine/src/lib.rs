//! Motor de aplicación: casos de uso compartidos por la CLI y la interfaz.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

mod error;

pub use error::{CaseField, EngineError, Stage};
pub use hm_core::PumpType;
pub use hm_design::{DesignReport, METHOD, PARAM_SPECS};
