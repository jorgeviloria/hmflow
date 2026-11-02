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

    pub fn mul(&self, other: &Self) -> Self {
        let dim = self.dim.mul(other.dim);
        Self {
            symbol: dim.symbol(),
            dim,
            factor: self.factor * other.factor,
            offset: 0.0,
        }
    }

    pub fn div(&self, other: &Self) -> Self {
        let dim = self.dim.div(other.dim);
        Self {
            symbol: dim.symbol(),
            dim,
            factor: self.factor / other.factor,
            offset: 0.0,
        }
    }

    pub fn powi(&self, exponent: i32) -> Self {
        let dim = self.dim.powf(exponent as f64);
        Self {
            symbol: dim.symbol(),
            dim,
            factor: self.factor.powi(exponent),
            offset: 0.0,
        }
    }

    pub fn compatible(&self, other: &Self) -> bool {
        self.dim == other.dim
    }
}
