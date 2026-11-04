//! Entrada del diseño y validación del caso.
//!
//! `validate_case` no añade reglas: une `design_input`, `DesignInput::validate`
//! y `DesignOptions::validate`, y pone el nombre del campo en el error.

use hm_design::{DesignInput, FieldError, default_mechanical_efficiency, validate_duty};

use crate::case::Case;
use crate::error::{CaseField, EngineError, input_error};
use crate::fluid::{kinematic_viscosity, npsha};

/// Construye la entrada del diseño.
///
/// Primero comprueba Q, H y n. La estimación de ηm necesita un caudal y una
/// velocidad con dimensión, y un error de ahí no debe salir como fallo de
/// fórmula. Después resuelve el NPSHa, la viscosidad cinemática y, si falta,
/// ηm.
pub fn design_input(case: &Case) -> Result<DesignInput, EngineError> {
    validate_duty(
        &case.duty.q.quantity,
        &case.duty.h.quantity,
        &case.duty.n.quantity,
    )
    .map_err(invalid_field)?;

    let npsha = npsha(&case.intake, &case.fluid)?;
    let nu = kinematic_viscosity(&case.fluid)?;
    let eta_m = match case.mechanical_efficiency {
        Some(eta_m) => eta_m,
        None => default_mechanical_efficiency(&case.duty.q.quantity, &case.duty.n.quantity)?,
    };

    Ok(DesignInput {
        q: case.duty.q.quantity.clone(),
        h: case.duty.h.quantity.clone(),
        n: case.duty.n.quantity.clone(),
        rho: case.fluid.density.value.clone(),
        eta_m,
        npsha,
        nu,
    })
}

/// `design_input`, la entrada del diseño y las opciones. Sin reglas propias.
pub fn validate_case(case: &Case) -> Result<(), EngineError> {
    let input = design_input(case)?;
    input.validate().map_err(invalid_field)?;
    case.design.validate().map_err(invalid_field)?;
    Ok(())
}

fn invalid_field(error: FieldError) -> EngineError {
    let field = match error.name {
        "Q" => CaseField::Q,
        "H" => CaseField::H,
        "n" => CaseField::N,
        "rho" => CaseField::Density,
        "NPSHa" => CaseField::Npsha,
        "eta_m" => CaseField::MechanicalEfficiency,
        name => CaseField::Param(name),
    };
    input_error(field, field_code(field), error.message)
}

fn field_code(field: CaseField) -> &'static str {
    match field {
        CaseField::Q => "HM-PROJ-002",
        CaseField::H => "HM-PROJ-003",
        CaseField::N => "HM-PROJ-004",
        CaseField::Npsha => "HM-PROJ-005",
        CaseField::Temperature => "HM-PROJ-006",
        CaseField::Viscosity => "HM-PROJ-026",
        CaseField::Suction => "HM-PROJ-027",
        CaseField::Density | CaseField::MechanicalEfficiency | CaseField::Param(_) => "HM-PROJ-020",
    }
}
