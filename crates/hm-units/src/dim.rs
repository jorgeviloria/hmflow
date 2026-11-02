//! Dimensiones físicas en las siete magnitudes del SI.

use std::fmt;

pub const N_DIMS: usize = 7;

const NAMES: [&str; N_DIMS] = ["m", "kg", "s", "K", "A", "mol", "cd"];

/// Exponentes de las siete magnitudes base.
#[derive(Clone, Copy, Debug)]
pub struct Dimension(pub [f64; N_DIMS]);

impl Dimension {
    pub const NONE: Self = Self([0.0; N_DIMS]);
    pub const LENGTH: Self = Self([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    pub const MASS: Self = Self([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    pub const TIME: Self = Self([0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    pub const TEMPERATURE: Self = Self([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
    pub const CURRENT: Self = Self([0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    pub const AMOUNT: Self = Self([0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    pub const LUMINOUS: Self = Self([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);

    pub const AREA: Self = Self([2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    pub const VOLUME: Self = Self([3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    pub const VELOCITY: Self = Self([1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]);
    pub const ACCELERATION: Self = Self([1.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0]);
    pub const FREQUENCY: Self = Self([0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]);
    pub const DENSITY: Self = Self([-3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    pub const FORCE: Self = Self([1.0, 1.0, -2.0, 0.0, 0.0, 0.0, 0.0]);
    pub const PRESSURE: Self = Self([-1.0, 1.0, -2.0, 0.0, 0.0, 0.0, 0.0]);
    pub const ENERGY: Self = Self([2.0, 1.0, -2.0, 0.0, 0.0, 0.0, 0.0]);
    pub const POWER: Self = Self([2.0, 1.0, -3.0, 0.0, 0.0, 0.0, 0.0]);

    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, other: Self) -> Self {
        let mut result = [0.0; N_DIMS];
        for (slot, (a, b)) in result.iter_mut().zip(self.0.iter().zip(other.0.iter())) {
            *slot = a + b;
        }
        Self(result)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn div(self, other: Self) -> Self {
        let mut result = [0.0; N_DIMS];
        for (slot, (a, b)) in result.iter_mut().zip(self.0.iter().zip(other.0.iter())) {
            *slot = a - b;
        }
        Self(result)
    }

    pub fn powf(self, exponent: f64) -> Self {
        let mut result = [0.0; N_DIMS];
        for (slot, value) in result.iter_mut().zip(self.0.iter()) {
            *slot = value * exponent;
        }
        Self(result)
    }

    pub fn powi(self, exponent: i32) -> Self {
        self.powf(exponent as f64)
    }

    pub fn is_none(self) -> bool {
        self.0.iter().all(|v| v.abs() < 1e-9)
    }

    /// Una magnitud física válida tiene exponentes enteros.
    pub fn is_integral(self) -> bool {
        self.0.iter().all(|v| (v - v.round()).abs() < 1e-9)
    }

    pub fn symbol(self) -> String {
        if self.is_none() {
            return "1".to_string();
        }
        if !self.is_integral() {
            return "SI".to_string();
        }

        let mut numerator = String::new();
        let mut denominator = String::new();

        for (index, exponent) in self.0.iter().enumerate() {
            let value = exponent.round() as i32;
            if value == 0 {
                continue;
            }
            let (target, power) = if value > 0 {
                (&mut numerator, value)
            } else {
                (&mut denominator, -value)
            };
            if !target.is_empty() {
                target.push('*');
            }
            target.push_str(NAMES[index]);
            if power != 1 {
                target.push_str(&power.to_string());
            }
        }

        if numerator.is_empty() {
            numerator.push('1');
        }
        if denominator.is_empty() {
            numerator
        } else {
            format!("{numerator}/{denominator}")
        }
    }
}

impl PartialEq for Dimension {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .all(|(a, b)| (a - b).abs() < 1e-9)
    }
}

impl Eq for Dimension {}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.symbol())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_is_force_over_area() {
        assert_eq!(Dimension::FORCE.div(Dimension::AREA), Dimension::PRESSURE);
    }

    #[test]
    fn density_symbol() {
        assert_eq!(Dimension::DENSITY.symbol(), "kg/m3");
    }

    #[test]
    fn power_symbol() {
        assert_eq!(Dimension::POWER.symbol(), "m2*kg/s3");
    }

    #[test]
    fn fractional_dimension_is_not_integral() {
        assert!(!Dimension::VOLUME.powf(0.5).is_integral());
    }
}
