//! Motor de aplicación: casos de uso compartidos por la CLI y la interfaz.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

mod case;
mod convert;
mod error;

pub use case::{
    Case, Density, DensitySource, Duty, Entered, Fluid, FluidKind, Intake, Temperature, Viscosity,
};
pub use convert::{MANUAL_PARAMS, ManualParam, case_from_project};
pub use error::{CaseField, EngineError, Stage};
pub use hm_core::PumpType;
pub use hm_design::{DesignReport, METHOD, PARAM_SPECS};
