use crate::dim::Dimension;

/// Una unidad con su dimensión, factor a SI y desplazamiento.
///
/// `to_si(v) = v * factor + offset`. El desplazamiento solo se usa en
/// temperaturas absolutas (°C, °F).
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub symbol: String,
    pub dim: Dimension,
    pub factor: f64,
    pub offset: f64,
}

impl Unit {
    pub fn new(symbol: impl Into<String>, dim: Dimension, factor: f64) -> Self {
        Self {
            symbol: symbol.into(),
            dim,
            factor,
            offset: 0.0,
        }
    }

    pub fn with_offset(mut self, offset: f64) -> Self {
        self.offset = offset;
        self
    }

    pub fn base(dim: Dimension) -> Self {
        Self {
            symbol: dim.symbol(),
            dim,
            factor: 1.0,
            offset: 0.0,
        }
    }

    pub fn dimensionless() -> Self {
        Self::new("1", Dimension::NONE, 1.0)
    }

    pub fn to_si(&self, value: f64) -> f64 {
        value * self.factor + self.offset
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn from_si(&self, si: f64) -> f64 {
        (si - self.offset) / self.factor
    }

    /// Producto de unidades. El símbolo conserva las unidades de partida
    /// (`kW*h`), porque el factor también las conserva.
    pub fn mul(&self, other: &Self) -> Self {
        Self {
            symbol: format!("{}*{}", self.symbol, other.symbol),
            dim: self.dim.mul(other.dim),
            factor: self.factor * other.factor,
            offset: 0.0,
        }
    }

    /// Cociente de unidades. Un divisor compuesto va entre paréntesis para
    /// que el símbolo se lea igual que se calculó: `kg/(m*s)`.
    pub fn div(&self, other: &Self) -> Self {
        Self {
            symbol: format!("{}/{}", self.symbol, grouped(&other.symbol)),
            dim: self.dim.div(other.dim),
            factor: self.factor / other.factor,
            offset: 0.0,
        }
    }

    /// Potencia entera. `m` al cubo se escribe `m3`, como lo acepta el
    /// parser; el resto usa `^`: `(m/s)^2`, `s^-1`.
    pub fn powi(&self, exponent: i32) -> Self {
        let symbol = if exponent > 0 && self.symbol.chars().all(|c| c.is_ascii_alphabetic()) {
            format!("{}{exponent}", self.symbol)
        } else {
            format!("{}^{exponent}", grouped(&self.symbol))
        };
        Self {
            symbol,
            dim: self.dim.powf(exponent as f64),
            factor: self.factor.powi(exponent),
            offset: 0.0,
        }
    }

    pub fn compatible(&self, other: &Self) -> bool {
        self.dim == other.dim
    }
}

/// Pone entre paréntesis un símbolo compuesto.
fn grouped(symbol: &str) -> String {
    if symbol.contains(['*', '/', '^']) {
        format!("({symbol})")
    } else {
        symbol.to_string()
    }
}
