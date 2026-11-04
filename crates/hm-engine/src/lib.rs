//! Motor de aplicación: casos de uso compartidos por la CLI y la interfaz.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

mod case;
mod convert;
mod error;
mod fluid;

pub use case::{
    Case, Density, DensitySource, Duty, Entered, Fluid, FluidKind, Intake, Temperature, Viscosity,
};
pub use convert::{
    Decode, MANUAL_PARAMS, ManualParam, add_fluid_temperature_diagnostics, case_from_project,
    kinematic_viscosity_of, project_viscosity_effect, suction_npsha,
};
pub use error::{CaseField, EngineError, Stage, cli_message};
pub use fluid::{
    fluid_diagnostics, fluid_kind, kinematic_viscosity, looks_like_water, npsh_fluid_diagnostics,
    npsha, parse_temperature, viscosity_effect, viscosity_from_text, water_density_at,
};
pub use hm_core::PumpType;
pub use hm_design::{DesignReport, METHOD, PARAM_SPECS};
