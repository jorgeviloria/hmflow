//! Unidades, dimensiones y cantidades con análisis dimensional.
//!
//! Los exponentes de dimensión son `f64` para permitir operaciones como
//! `sqrt` y potencias fraccionarias durante el cálculo. Una magnitud física
//! válida debe terminar con exponentes enteros.

mod dim;
mod error;
mod parse;
mod quantity;
mod registry;
mod unit;

pub use dim::{Dimension, N_DIMS};
pub use error::UnitError;
pub use parse::{parse_quantity, parse_unit};
pub use quantity::Quantity;
pub use registry::lookup;
pub use unit::Unit;

/// Constantes físicas con unidades.
pub mod constants {
    use crate::quantity::Quantity;
    use crate::unit::Unit;
    use crate::{Dimension, parse_quantity};

    /// Gravedad estándar.
    pub fn g() -> Quantity {
        Quantity::new(
            9.806_65,
            Unit::new("m/s2", Dimension::VELOCITY.div(Dimension::TIME), 1.0),
        )
    }

    /// Presión atmosférica estándar.
    pub fn p_atm() -> Quantity {
        parse_quantity("101325 Pa").expect("constante válida")
    }

    /// Densidad del agua a 20 °C, referencia.
    pub fn rho_water_20c() -> Quantity {
        parse_quantity("998.2 kg/m3").expect("constante válida")
    }
}
