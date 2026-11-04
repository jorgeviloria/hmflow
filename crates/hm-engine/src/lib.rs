//! Motor de aplicación: casos de uso compartidos por la CLI y la interfaz.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

mod analysis;
mod audit;
mod case;
mod changes;
mod convert;
mod dispatch;
mod error;
mod fluid;
mod project_ops;
mod recalc;
mod validate;

pub use analysis::{Analysis, CurvePoints};
pub use audit::{audit, audit_with_analysis};
pub use case::{
    Case, Density, DensitySource, Duty, Entered, Fluid, FluidKind, Intake, Temperature, Viscosity,
};
pub use changes::{ChangeKind, ParamChange};
pub use convert::{
    Decode, MANUAL_PARAMS, MAX_COUNT, ManualDefaults, ManualParam, case_from_project, design_params,
};
pub use dispatch::analyze;
pub use error::{CaseField, EngineError, Stage, cli_message};
pub use fluid::{
    fluid_diagnostics, fluid_kind, kinematic_viscosity, looks_like_water, npsh_fluid_diagnostics,
    npsha, parse_temperature, viscosity_effect, viscosity_from_text, water_density_at,
};
pub use hm_core::PumpType;
pub use hm_design::{
    DesignOptions, DesignReport, METHOD, PARAM_SPECS, PREFERRED_OPERATING_REGION, REPORT_FORMAT,
};
pub use project_ops::{new_project, report};
pub use recalc::{Recalculated, recalc};
pub use validate::{design_input, validate_case};
