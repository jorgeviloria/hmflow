//! Fluido y aspiración del caso.
//!
//! El cálculo se movió desde la CLI. Las correlaciones del agua siguen en
//! `hm_design::fluid`; aquí solo se usan.

use std::collections::BTreeMap;

use hm_core::{Diagnostic, Diagnostics};
use hm_design::fluid::{
    WATER_DENSITY_RANGE_C, water_density, water_kinematic_viscosity, water_vapor_pressure,
};
use hm_design::{
    DesignError, DesignInput, DesignResult, ViscousCorrection, viscous_correction,
    viscous_diagnostics,
};
use hm_formula::Registry;
use hm_units::{Dimension, Quantity, parse_quantity, parse_unit};

use crate::case::{Fluid, FluidKind, Intake, Temperature, Viscosity};
use crate::error::{CaseField, EngineError, VAPOR_PRESSURE_RANGE, input_error};

/// Diferencia relativa admitida entre `rho` y la densidad del agua.
const WATER_DENSITY_TOLERANCE: f64 = 0.005;

/// Temperatura del agua cuando no se escribió ninguna, K (20 °C).
pub(crate) const DEFAULT_WATER_TEMPERATURE_K: f64 = 293.15;

/// El agua es más densa a 4 °C.
const DENSEST_WATER_K: f64 = 277.15;

/// Densidad dentro del rango del agua líquida entre 0 y 150 °C.
///
/// Sirve para suponer el fluido cuando no se indica: una densidad de agua a
/// otra temperatura se trata como agua, para que `HM-DES-009` la señale.
pub fn looks_like_water(rho_si: f64) -> bool {
    let hottest = WATER_DENSITY_RANGE_C.1 + 273.15;
    let (Some(lightest), Some(densest)) = (water_density(hottest), water_density(DENSEST_WATER_K))
    else {
        return false;
    };
    (lightest * (1.0 - WATER_DENSITY_TOLERANCE)..=densest * (1.0 + WATER_DENSITY_TOLERANCE))
        .contains(&rho_si)
}

/// Temperatura a partir del texto de la CLI o del proyecto.
pub fn parse_temperature(text: &str) -> Result<Temperature, EngineError> {
    let quantity = parse_quantity(text)
        .map_err(|error| input_error(CaseField::Temperature, "HM-PROJ-006", error.to_string()))?;
    if quantity.dim() != Dimension::TEMPERATURE {
        return Err(input_error(
            CaseField::Temperature,
            "HM-PROJ-006",
            format!("la temperatura debe ser una temperatura: {text}"),
        ));
    }
    Ok(Temperature {
        text: text.to_string(),
        kelvin: quantity.si,
    })
}

/// `water` o vacío es agua. Cualquier otro nombre se conserva.
pub fn fluid_kind(fluid: &str) -> FluidKind {
    if fluid.is_empty() || fluid.eq_ignore_ascii_case("water") {
        FluidKind::Water
    } else {
        FluidKind::Custom(fluid.to_string())
    }
}

/// Densidad del agua a esa temperatura, o `None` fuera de 0..150 °C.
///
/// El texto del error lo pone el adaptador. La CLI conserva «indica --rho».
pub fn water_density_at(temperature: &Temperature) -> Option<Quantity> {
    let value = water_density(temperature.kelvin)?;
    let unit = parse_unit("kg/m3").ok()?;
    Some(Quantity::from_si(value, unit))
}

/// Viscosidad ya interpretada, o `None` si el texto está vacío.
pub fn viscosity_from_text(text: &str) -> Result<Option<Viscosity>, EngineError> {
    if text.trim().is_empty() {
        return Ok(None);
    }

    let entered = crate::case::Entered::parse(text)
        .map_err(|error| input_error(CaseField::Viscosity, "HM-PROJ-026", error.to_string()))?;
    let kinematic = parse_unit("m2/s")
        .map_err(|error| input_error(CaseField::Viscosity, "HM-PROJ-026", error.to_string()))?;
    let dynamic = parse_unit("Pa*s")
        .map_err(|error| input_error(CaseField::Viscosity, "HM-PROJ-026", error.to_string()))?;

    let positive = entered.quantity.si.is_finite() && entered.quantity.si > 0.0;
    if entered.quantity.dim() != kinematic.dim && entered.quantity.dim() != dynamic.dim {
        return Err(input_error(
            CaseField::Viscosity,
            "HM-PROJ-026",
            format!("la viscosidad debe ser m2/s (cSt) o Pa*s (cP): {text}"),
        ));
    }
    if !positive {
        return Err(input_error(
            CaseField::Viscosity,
            "HM-PROJ-026",
            format!("la viscosidad debe ser positiva: {text}"),
        ));
    }
    if entered.quantity.dim() == kinematic.dim {
        Ok(Some(Viscosity::Kinematic(entered)))
    } else {
        Ok(Some(Viscosity::Dynamic(entered)))
    }
}

/// Coherencia entre fluido, temperatura y densidad.
///
/// Solo se puede comprobar el agua: su densidad se calcula con Kell (1975)
/// entre 0 y 150 °C. Un texto de temperatura inválido no llega aquí: lo
/// rechaza quien construye el `Fluid`.
pub fn fluid_diagnostics(fluid: &Fluid) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    if !matches!(fluid.kind, FluidKind::Water) {
        return diagnostics;
    }
    let temperature_k = fluid
        .temperature
        .as_ref()
        .map(|temperature| temperature.kelvin)
        .unwrap_or(DEFAULT_WATER_TEMPERATURE_K);
    let celsius = temperature_k - 273.15;
    let rho_si = fluid.density.value.si;

    match water_density(temperature_k) {
        None => diagnostics.push(
            Diagnostic::warning(
                "HM-DES-010",
                format!(
                    "el agua a {celsius:.1} °C queda fuera de la correlación de densidad (0..150 °C); \
                     no se comprobó rho"
                ),
            )
            .with_hint("Comprueba que rho corresponda a la temperatura de operación."),
        ),
        Some(expected) if ((rho_si - expected) / expected).abs() > WATER_DENSITY_TOLERANCE => {
            diagnostics.push(
                Diagnostic::warning(
                    "HM-DES-009",
                    format!(
                        "rho = {rho_si:.1} kg/m3 no es la densidad del agua a {celsius:.1} °C \
                         ({expected:.1} kg/m3)"
                    ),
                )
                .with_hint("Corrige rho o, si el fluido no es agua, usa fluid = \"custom\"."),
            );
        }
        Some(_) => {}
    }
    diagnostics
}

/// El NPSHA necesario de `hm-design` usa el factor de fluido del agua
/// (FF = 1, Gülich 2020, tabla 6.2). Con otro fluido se avisa.
pub fn npsh_fluid_diagnostics(fluid: &Fluid, npsh_checked: bool) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    if let FluidKind::Custom(name) = &fluid.kind
        && npsh_checked
    {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-018",
                format!(
                    "el NPSHA necesario supone agua (FF = 1) y el fluido es `{name}`; \
                     la tabla 6.2 de Gülich pide FF ≥ 1,15 con agua de mar o fluidos corrosivos \
                     y otros criterios con hidrocarburos"
                ),
            )
            .with_hint("Aumenta el NPSHA en consecuencia o confirma el NPSHR con el fabricante."),
        );
    }
    diagnostics
}

/// NPSHa de la aspiración.
///
/// `Intake::None` no tiene NPSHa. `Available` devuelve el valor escrito.
/// En la instalación, la altura y las pérdidas ausentes valen cero y la
/// presión de vapor ausente es la del agua a la temperatura (20 °C si no se
/// indicó). Si el fluido no es agua y falta, el error es `HM-PROJ-029`; si la
/// temperatura cae fuera de IAPWS, `HM-PROJ-027` sobre `[suction]`.
pub fn npsha(intake: &Intake, fluid: &Fluid) -> Result<Option<Quantity>, EngineError> {
    match intake {
        Intake::None => Ok(None),
        Intake::Available(entered) => Ok(Some(entered.quantity.clone())),
        Intake::Installation {
            pressure,
            height,
            losses,
            vapor_pressure,
        } => {
            let vapor = match vapor_pressure {
                Some(entered) => entered.quantity.clone(),
                None => water_vapor(fluid)?,
            };
            let inputs = [
                ("p_s", pressure.quantity.clone()),
                ("p_v", vapor),
                ("rho", si_quantity(fluid.density.value.si, "kg/m3")?),
                ("z", metres_or_zero(height.as_ref())?),
                ("hf", metres_or_zero(losses.as_ref())?),
            ]
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect::<BTreeMap<_, _>>();
            Ok(Some(
                Registry::all()
                    .evaluate("npsh.npsha", &inputs)
                    .map_err(design_error)?,
            ))
        }
    }
}

/// Viscosidad cinemática del fluido en m²/s.
///
/// La indicada (cinemática, o dinámica dividida por ρ) o la del agua a su
/// temperatura. `None` si el fluido no es agua y no se indicó, o si la
/// temperatura del agua cae fuera de la correlación.
pub fn kinematic_viscosity(fluid: &Fluid) -> Result<Option<f64>, EngineError> {
    match &fluid.viscosity {
        Some(Viscosity::Kinematic(entered)) => Ok(Some(entered.quantity.si)),
        Some(Viscosity::Dynamic(entered)) => Ok(Some(entered.quantity.si / fluid.density.value.si)),
        None if matches!(fluid.kind, FluidKind::Water) => {
            let temperature_k = fluid
                .temperature
                .as_ref()
                .map(|temperature| temperature.kelvin)
                .unwrap_or(DEFAULT_WATER_TEMPERATURE_K);
            Ok(water_kinematic_viscosity(temperature_k))
        }
        None => Ok(None),
    }
}

/// Corrección por viscosidad y sus avisos. Sin viscosidad conocida de un
/// fluido que no es agua, informa de que no se evaluó (`HM-DES-022`).
pub fn viscosity_effect(
    fluid: &Fluid,
    input: &DesignInput,
    result: &DesignResult,
) -> Result<(Diagnostics, Option<ViscousCorrection>), EngineError> {
    let mut diagnostics = Diagnostics::new();
    match kinematic_viscosity(fluid)? {
        Some(nu) => {
            let correction = viscous_correction(input, result, nu).map_err(design_error)?;
            diagnostics.extend(viscous_diagnostics(&correction, result.nq).into_vec());
            Ok((diagnostics, Some(correction)))
        }
        None => {
            if let FluidKind::Custom(name) = &fluid.kind {
                diagnostics.push(
                    Diagnostic::info(
                        "HM-DES-022",
                        format!(
                            "el fluido es `{name}` y no tiene viscosidad: no se evaluó su efecto"
                        ),
                    )
                    .with_hint("Indica la viscosidad en [operating] (viscosity = \"30 cSt\")."),
                );
            }
            Ok((diagnostics, None))
        }
    }
}

fn water_vapor(fluid: &Fluid) -> Result<Quantity, EngineError> {
    let FluidKind::Custom(name) = &fluid.kind else {
        let temperature_k = fluid
            .temperature
            .as_ref()
            .map(|temperature| temperature.kelvin)
            .unwrap_or(DEFAULT_WATER_TEMPERATURE_K);
        let pascal = water_vapor_pressure(temperature_k)
            .ok_or_else(|| input_error(CaseField::Suction, "HM-PROJ-027", VAPOR_PRESSURE_RANGE))?;
        return si_quantity(pascal, "Pa");
    };
    Err(input_error(
        CaseField::Suction,
        "HM-PROJ-029",
        format!("el fluido es `{name}`: indica la presión de vapor de la aspiración"),
    ))
}

/// Altura o pérdidas ausentes: la misma cantidad que `parse_quantity("0 m")`.
fn metres_or_zero(entered: Option<&crate::case::Entered>) -> Result<Quantity, EngineError> {
    match entered {
        Some(entered) => Ok(entered.quantity.clone()),
        None => parse_quantity("0 m").map_err(design_error),
    }
}

fn si_quantity(si: f64, unit: &str) -> Result<Quantity, EngineError> {
    Ok(Quantity::from_si(
        si,
        parse_unit(unit).map_err(design_error)?,
    ))
}

fn design_error(error: impl Into<DesignError>) -> EngineError {
    EngineError::Design(error.into())
}
