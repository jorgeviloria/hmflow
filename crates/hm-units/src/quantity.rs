use crate::dim::Dimension;
use crate::error::UnitError;
use crate::unit::Unit;

/// Un valor con unidades. Se guarda siempre en SI; la unidad es de presentación.
#[derive(Clone, Debug)]
pub struct Quantity {
    pub si: f64,
    pub unit: Unit,
}

impl Quantity {
    pub fn new(value: f64, unit: Unit) -> Self {
        let si = unit.to_si(value);
        Self { si, unit }
    }

    pub fn from_si(si: f64, unit: Unit) -> Self {
        Self { si, unit }
    }

    pub fn value(&self) -> f64 {
        self.unit.from_si(self.si)
    }

    pub fn dim(&self) -> Dimension {
        self.unit.dim
    }

    pub fn is_dimensionless(&self) -> bool {
        self.dim().is_none()
    }

    pub fn require_same(&self, other: &Self) -> Result<(), UnitError> {
        if self.dim() == other.dim() {
            Ok(())
        } else {
            Err(UnitError::DimensionMismatch {
                left: self.dim().symbol(),
                right: other.dim().symbol(),
            })
        }
    }

    pub fn add(&self, other: &Self) -> Result<Self, UnitError> {
        self.require_same(other)?;
        Ok(Self {
            si: self.si + other.si,
            unit: self.unit.clone(),
        })
    }

    pub fn sub(&self, other: &Self) -> Result<Self, UnitError> {
        self.require_same(other)?;
        Ok(Self {
            si: self.si - other.si,
            unit: self.unit.clone(),
        })
    }

    pub fn neg(&self) -> Self {
        Self {
            si: -self.si,
            unit: self.unit.clone(),
        }
    }

    pub fn mul(&self, other: &Self) -> Self {
        let dim = self.dim().mul(other.dim());
        Self {
            si: self.si * other.si,
            unit: Unit::base(dim),
        }
    }

    pub fn div(&self, other: &Self) -> Result<Self, UnitError> {
        if other.si == 0.0 {
            return Err(UnitError::DivisionByZero);
        }
        let dim = self.dim().div(other.dim());
        Ok(Self {
            si: self.si / other.si,
            unit: Unit::base(dim),
        })
    }

    pub fn powi(&self, exponent: i32) -> Self {
        let dim = self.dim().powf(exponent as f64);
        Self {
            si: self.si.powi(exponent),
            unit: Unit::base(dim),
        }
    }

    pub fn powf(&self, exponent: f64) -> Self {
        let dim = self.dim().powf(exponent);
        Self {
            si: self.si.powf(exponent),
            unit: Unit::base(dim),
        }
    }

    pub fn sqrt(&self) -> Self {
        self.powf(0.5)
    }

    pub fn abs(&self) -> Self {
        Self {
            si: self.si.abs(),
            unit: self.unit.clone(),
        }
    }

    pub fn to(&self, unit: &Unit) -> Result<Self, UnitError> {
        if self.dim() != unit.dim {
            return Err(UnitError::DimensionMismatch {
                left: self.dim().symbol(),
                right: unit.dim.symbol(),
            });
        }
        Ok(Self::from_si(self.si, unit.clone()))
    }

    pub fn ensure_physical(&self) -> Result<(), UnitError> {
        if self.dim().is_integral() {
            Ok(())
        } else {
            Err(UnitError::NonPhysical(self.dim().symbol()))
        }
    }
}
