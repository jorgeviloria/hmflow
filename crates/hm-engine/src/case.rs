//! Modelo tipado de las entradas del caso.

use hm_core::PumpType;
use hm_design::DesignOptions;
use hm_units::{Quantity, UnitError, parse_quantity};

/// Texto original de una entrada y su cantidad resuelta.
#[derive(Debug, Clone)]
pub struct Entered {
    pub text: String,
    pub quantity: Quantity,
}

impl Entered {
    pub fn parse(text: &str) -> Result<Self, UnitError> {
        Ok(Self {
            text: text.to_string(),
            quantity: parse_quantity(text)?,
        })
    }
}

/// Temperatura con su texto original y su valor absoluto en kelvin.
#[derive(Debug, Clone)]
pub struct Temperature {
    pub text: String,
    pub kelvin: f64,
}

/// Punto de operación.
#[derive(Debug, Clone)]
pub struct Duty {
    pub q: Entered,
    pub h: Entered,
    pub n: Entered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FluidKind {
    Water,
    Custom(String),
}

#[derive(Debug, Clone)]
pub enum DensitySource {
    Explicit {
        text: String,
    },
    /// Agua a la temperatura que escribió el usuario.
    WaterAtTemperature,
    /// Agua sin temperatura escrita: se toma a 20 °C.
    WaterAtDefaultTemperature,
    /// No se pudo derivar ni está escrita: 998,2 kg/m3.
    Fallback,
}

#[derive(Debug, Clone)]
pub struct Density {
    pub value: Quantity,
    pub source: DensitySource,
}

#[derive(Debug, Clone)]
pub enum Viscosity {
    Kinematic(Entered),
    Dynamic(Entered),
}

#[derive(Debug, Clone)]
pub struct Fluid {
    pub kind: FluidKind,
    pub temperature: Option<Temperature>,
    pub density: Density,
    pub viscosity: Option<Viscosity>,
}

// El caso es un modelo de datos, no una ruta caliente; la variante de
// instalación es la más grande y no compensa boxear sus campos.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Intake {
    None,
    Available(Entered),
    Installation {
        pressure: Entered,
        height: Option<Entered>,
        losses: Option<Entered>,
        vapor_pressure: Option<Entered>,
    },
}

/// Entradas completas de un análisis, sin texto de auditoría ni modos.
#[derive(Debug, Clone)]
pub struct Case {
    pub pump_type: PumpType,
    pub duty: Duty,
    pub fluid: Fluid,
    pub intake: Intake,
    pub mechanical_efficiency: Option<f64>,
    pub design: DesignOptions,
}
