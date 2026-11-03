//! Utilidades numéricas: interpolación y resolución de raíces.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

use thiserror::Error;

/// Errores de cálculo numérico.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum MathError {
    #[error("el intervalo no contiene una raíz: f(a) y f(b) tienen el mismo signo")]
    NoBracket,

    #[error("no converge tras {0} iteraciones")]
    NotConverged(usize),

    #[error("derivada nula")]
    ZeroDerivative,
}

/// Interpolación lineal.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Limita un valor a un intervalo.
pub fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.max(low).min(high)
}

/// Bisección con tolerancia absoluta.
pub fn bisect<F>(
    mut f: F,
    mut a: f64,
    mut b: f64,
    tolerance: f64,
    max_iterations: usize,
) -> Result<f64, MathError>
where
    F: FnMut(f64) -> f64,
{
    let mut fa = f(a);
    if fa == 0.0 {
        return Ok(a);
    }
    let fb = f(b);
    if fb == 0.0 {
        return Ok(b);
    }

    if fa * fb > 0.0 {
        return Err(MathError::NoBracket);
    }

    for _ in 0..max_iterations {
        let middle = 0.5 * (a + b);
        let fm = f(middle);

        if fm.abs() < tolerance || (b - a).abs() < tolerance {
            return Ok(middle);
        }

        if fa * fm < 0.0 {
            b = middle;
        } else {
            a = middle;
            fa = fm;
        }
    }

    Err(MathError::NotConverged(max_iterations))
}

/// Newton-Raphson con tolerancia absoluta sobre `f`.
pub fn newton<F, DF>(
    mut f: F,
    mut derivative: DF,
    mut x: f64,
    tolerance: f64,
    max_iterations: usize,
) -> Result<f64, MathError>
where
    F: FnMut(f64) -> f64,
    DF: FnMut(f64) -> f64,
{
    for _ in 0..max_iterations {
        let fx = f(x);
        if fx.abs() < tolerance {
            return Ok(x);
        }

        let slope = derivative(x);
        if slope.abs() < f64::EPSILON {
            return Err(MathError::ZeroDerivative);
        }

        x -= fx / slope;
    }

    Err(MathError::NotConverged(max_iterations))
}

/// Media aritmética de una serie no vacía.
pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_endpoints() {
        assert!((lerp(2.0, 4.0, 0.0) - 2.0).abs() < 1e-12);
        assert!((lerp(2.0, 4.0, 1.0) - 4.0).abs() < 1e-12);
        assert!((lerp(2.0, 4.0, 0.5) - 3.0).abs() < 1e-12);
    }

    #[test]
    fn bisect_square_root_of_two() {
        let root = bisect(|x| x * x - 2.0, 0.0, 2.0, 1e-12, 200).unwrap();
        assert!((root - std::f64::consts::SQRT_2).abs() < 1e-9);
    }

    #[test]
    fn bisect_finds_root_at_left_endpoint() {
        let root = bisect(|x| x, 0.0, 1.0, 1e-12, 100).unwrap();
        assert_eq!(root, 0.0);
    }

    #[test]
    fn bisect_finds_root_at_right_endpoint() {
        let root = bisect(|x| x - 1.0, 0.0, 1.0, 1e-12, 100).unwrap();
        assert_eq!(root, 1.0);
    }

    #[test]
    fn bisect_without_bracket_fails() {
        assert_eq!(
            bisect(|x| x * x + 1.0, -1.0, 1.0, 1e-9, 50),
            Err(MathError::NoBracket)
        );
    }

    #[test]
    fn newton_square_root_of_two() {
        let root = newton(|x| x * x - 2.0, |x| 2.0 * x, 1.0, 1e-12, 50).unwrap();
        assert!((root - std::f64::consts::SQRT_2).abs() < 1e-9);
    }

    #[test]
    fn newton_zero_derivative_fails() {
        assert_eq!(
            newton(|_| 1.0, |_| 0.0, 1.0, 1e-9, 10),
            Err(MathError::ZeroDerivative)
        );
    }

    #[test]
    fn clamp_limits() {
        assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(-5.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn mean_of_empty_is_none() {
        assert_eq!(mean(&[]), None);
        assert_eq!(mean(&[1.0, 2.0, 3.0]), Some(2.0));
    }
}
