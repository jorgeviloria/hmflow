//! Análisis dimensional y evaluación.

use std::collections::BTreeMap;

use hm_units::{Dimension, Quantity, Unit};

use crate::ast::{BinOp, Expr, UnOp};
use crate::error::FormulaError;

/// Constantes conocidas por el DSL.
pub fn constant(name: &str) -> Option<Quantity> {
    match name {
        "pi" => Some(Quantity::new(std::f64::consts::PI, Unit::dimensionless())),
        "e" => Some(Quantity::new(std::f64::consts::E, Unit::dimensionless())),
        "g" => Some(Quantity::new(
            9.806_65,
            Unit::new("m/s2", Dimension::ACCELERATION, 1.0),
        )),
        _ => None,
    }
}

/// Deduce la dimensión de una expresión.
pub fn infer(expr: &Expr, env: &BTreeMap<String, Dimension>) -> Result<Dimension, FormulaError> {
    match expr {
        Expr::Number(_) => Ok(Dimension::NONE),
        Expr::Var(name) => {
            if let Some(dim) = env.get(name) {
                return Ok(*dim);
            }
            if let Some(value) = constant(name) {
                return Ok(value.dim());
            }
            Err(FormulaError::UnknownVariable(name.clone()))
        }
        Expr::Unary(_, inner) => infer(inner, env),
        Expr::Binary(op, left, right) => {
            let dl = infer(left, env)?;
            let dr = infer(right, env)?;
            match op {
                BinOp::Add | BinOp::Sub => {
                    if dl != dr {
                        return Err(FormulaError::DimensionMismatch {
                            context: "suma o resta".to_string(),
                            left: dl.symbol(),
                            right: dr.symbol(),
                        });
                    }
                    Ok(dl)
                }
                BinOp::Mul => Ok(dl.mul(dr)),
                BinOp::Div => Ok(dl.div(dr)),
                BinOp::Pow => {
                    let exponent = const_eval(right).ok_or(FormulaError::NonConstantExponent)?;
                    Ok(dl.powf(exponent))
                }
            }
        }
        Expr::Call(name, args) => infer_call(name, args, env),
    }
}

/// Evalúa una expresión con un entorno de cantidades.
pub fn eval(expr: &Expr, env: &BTreeMap<String, Quantity>) -> Result<Quantity, FormulaError> {
    match expr {
        Expr::Number(value) => Ok(Quantity::new(*value, Unit::dimensionless())),
        Expr::Var(name) => env
            .get(name)
            .cloned()
            .or_else(|| constant(name))
            .ok_or_else(|| FormulaError::UnknownVariable(name.clone())),
        Expr::Unary(UnOp::Neg, inner) => Ok(eval(inner, env)?.neg()),
        Expr::Unary(UnOp::Pos, inner) => eval(inner, env),
        Expr::Binary(op, left, right) => {
            let a = eval(left, env)?;
            let b = eval(right, env)?;
            match op {
                BinOp::Add => a.add(&b).map_err(FormulaError::from),
                BinOp::Sub => a.sub(&b).map_err(FormulaError::from),
                BinOp::Mul => Ok(a.mul(&b)),
                BinOp::Div => a.div(&b).map_err(FormulaError::from),
                BinOp::Pow => {
                    let exponent = const_eval(right).ok_or(FormulaError::NonConstantExponent)?;
                    if (exponent - exponent.round()).abs() < 1e-9 {
                        Ok(a.powi(exponent.round() as i32))
                    } else {
                        Ok(a.powf(exponent))
                    }
                }
            }
        }
        Expr::Call(name, args) => eval_call(name, args, env),
    }
}

fn infer_call(
    name: &str,
    args: &[Expr],
    env: &BTreeMap<String, Dimension>,
) -> Result<Dimension, FormulaError> {
    match name {
        "sqrt" => {
            arity(name, args, 1)?;
            Ok(infer(&args[0], env)?.powf(0.5))
        }
        "abs" => {
            arity(name, args, 1)?;
            infer(&args[0], env)
        }
        "exp" | "ln" | "log10" | "sin" | "cos" | "tan" | "atan" => {
            arity(name, args, 1)?;
            require_dimensionless(name, infer(&args[0], env)?)?;
            Ok(Dimension::NONE)
        }
        "min" | "max" => {
            arity(name, args, 2)?;
            let a = infer(&args[0], env)?;
            let b = infer(&args[1], env)?;
            if a != b {
                return Err(FormulaError::DimensionMismatch {
                    context: name.to_string(),
                    left: a.symbol(),
                    right: b.symbol(),
                });
            }
            Ok(a)
        }
        "nq" => {
            arity(name, args, 3)?;
            let n = infer(&args[0], env)?;
            let q = infer(&args[1], env)?;
            let h = infer(&args[2], env)?;
            expect_dimension(name, "n", n, Dimension::FREQUENCY)?;
            expect_dimension(name, "Q", q, Dimension::VOLUME.div(Dimension::TIME))?;
            expect_dimension(name, "H", h, Dimension::LENGTH)?;
            Ok(Dimension::NONE)
        }
        _ => Err(FormulaError::UnknownFunction(name.to_string())),
    }
}

fn eval_call(
    name: &str,
    args: &[Expr],
    env: &BTreeMap<String, Quantity>,
) -> Result<Quantity, FormulaError> {
    match name {
        "sqrt" => {
            arity(name, args, 1)?;
            Ok(eval(&args[0], env)?.sqrt())
        }
        "abs" => {
            arity(name, args, 1)?;
            Ok(eval(&args[0], env)?.abs())
        }
        "exp" => {
            arity(name, args, 1)?;
            let value = dimensionless_arg(name, eval(&args[0], env)?)?;
            Ok(Quantity::new(value.si.exp(), Unit::dimensionless()))
        }
        "ln" => {
            arity(name, args, 1)?;
            let value = dimensionless_arg(name, eval(&args[0], env)?)?;
            Ok(Quantity::new(value.si.ln(), Unit::dimensionless()))
        }
        "log10" => {
            arity(name, args, 1)?;
            let value = dimensionless_arg(name, eval(&args[0], env)?)?;
            Ok(Quantity::new(value.si.log10(), Unit::dimensionless()))
        }
        "sin" => {
            arity(name, args, 1)?;
            let value = dimensionless_arg(name, eval(&args[0], env)?)?;
            Ok(Quantity::new(value.si.sin(), Unit::dimensionless()))
        }
        "cos" => {
            arity(name, args, 1)?;
            let value = dimensionless_arg(name, eval(&args[0], env)?)?;
            Ok(Quantity::new(value.si.cos(), Unit::dimensionless()))
        }
        "tan" => {
            arity(name, args, 1)?;
            let value = dimensionless_arg(name, eval(&args[0], env)?)?;
            Ok(Quantity::new(value.si.tan(), Unit::dimensionless()))
        }
        "atan" => {
            arity(name, args, 1)?;
            let value = dimensionless_arg(name, eval(&args[0], env)?)?;
            Ok(Quantity::new(value.si.atan(), Unit::dimensionless()))
        }
        "min" | "max" => {
            arity(name, args, 2)?;
            let a = eval(&args[0], env)?;
            let b = eval(&args[1], env)?;
            a.require_same(&b)?;
            let take_a = if name == "min" {
                a.si <= b.si
            } else {
                a.si >= b.si
            };
            Ok(if take_a { a } else { b })
        }
        "nq" => {
            arity(name, args, 3)?;
            let n = eval(&args[0], env)?;
            let q = eval(&args[1], env)?;
            let h = eval(&args[2], env)?;
            expect_dimension(name, "n", n.dim(), Dimension::FREQUENCY)?;
            expect_dimension(name, "Q", q.dim(), Dimension::VOLUME.div(Dimension::TIME))?;
            expect_dimension(name, "H", h.dim(), Dimension::LENGTH)?;
            // Convención europea: n en min^-1, Q en m3/s, H en m.
            let value = 60.0 * n.si * q.si.sqrt() / h.si.powf(0.75);
            Ok(Quantity::new(value, Unit::dimensionless()))
        }
        _ => Err(FormulaError::UnknownFunction(name.to_string())),
    }
}

fn arity(name: &str, args: &[Expr], expected: usize) -> Result<(), FormulaError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(FormulaError::Arity {
            function: name.to_string(),
            expected,
            got: args.len(),
        })
    }
}

fn require_dimensionless(function: &str, dim: Dimension) -> Result<(), FormulaError> {
    if dim.is_none() {
        Ok(())
    } else {
        Err(FormulaError::DimensionMismatch {
            context: function.to_string(),
            left: dim.symbol(),
            right: "1".to_string(),
        })
    }
}

fn dimensionless_arg(function: &str, value: Quantity) -> Result<Quantity, FormulaError> {
    require_dimensionless(function, value.dim())?;
    Ok(value)
}

fn expect_dimension(
    function: &str,
    argument: &str,
    actual: Dimension,
    expected: Dimension,
) -> Result<(), FormulaError> {
    if actual == expected {
        Ok(())
    } else {
        Err(FormulaError::DimensionMismatch {
            context: format!("{function}({argument})"),
            left: actual.symbol(),
            right: expected.symbol(),
        })
    }
}

fn const_eval(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Number(value) => Some(*value),
        Expr::Unary(UnOp::Neg, inner) => Some(-const_eval(inner)?),
        Expr::Unary(UnOp::Pos, inner) => const_eval(inner),
        Expr::Binary(op, left, right) => {
            let a = const_eval(left)?;
            let b = const_eval(right)?;
            match op {
                BinOp::Add => Some(a + b),
                BinOp::Sub => Some(a - b),
                BinOp::Mul => Some(a * b),
                BinOp::Div => {
                    if b == 0.0 {
                        None
                    } else {
                        Some(a / b)
                    }
                }
                BinOp::Pow => Some(a.powf(b)),
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use hm_units::parse_quantity;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, Quantity> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), parse_quantity(v).unwrap()))
            .collect()
    }

    #[test]
    fn evaluates_sum() {
        let expr = parse("a + b").unwrap();
        let values = env(&[("a", "1 m"), ("b", "50 cm")]);
        let result = eval(&expr, &values).unwrap();
        assert!((result.value() - 1.5).abs() < 1e-12);
    }

    #[test]
    fn rejects_incompatible_sum() {
        let expr = parse("a + b").unwrap();
        let values = env(&[("a", "1 m"), ("b", "1 s")]);
        assert!(eval(&expr, &values).is_err());
    }

    #[test]
    fn evaluates_power() {
        let expr = parse("a^2").unwrap();
        let values = env(&[("a", "3 m")]);
        let result = eval(&expr, &values).unwrap();
        assert!((result.si - 9.0).abs() < 1e-12);
        assert_eq!(result.dim(), Dimension::AREA);
    }

    #[test]
    fn evaluates_sqrt_of_area() {
        let expr = parse("sqrt(a)").unwrap();
        let values = env(&[("a", "9 m2")]);
        let result = eval(&expr, &values).unwrap();
        assert!((result.si - 3.0).abs() < 1e-12);
        assert_eq!(result.dim(), Dimension::LENGTH);
    }

    #[test]
    fn sqrt_of_volume_is_not_physical() {
        let expr = parse("sqrt(a)").unwrap();
        let values = env(&[("a", "9 m3")]);
        let result = eval(&expr, &values).unwrap();
        assert!(result.ensure_physical().is_err());
    }

    #[test]
    fn constants_work() {
        let expr = parse("pi").unwrap();
        let values = BTreeMap::new();
        let result = eval(&expr, &values).unwrap();
        assert!((result.si - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn gravity_has_acceleration_dimension() {
        let expr = parse("g").unwrap();
        let values = BTreeMap::new();
        let result = eval(&expr, &values).unwrap();
        assert_eq!(result.dim(), Dimension::ACCELERATION);
    }

    #[test]
    fn nq_returns_dimensionless() {
        let expr = parse("nq(n, Q, H)").unwrap();
        let values = env(&[("n", "2900 rpm"), ("Q", "120 m3/h"), ("H", "45 m")]);
        let result = eval(&expr, &values).unwrap();
        assert!(result.dim().is_none());
        assert!((result.si - 30.474).abs() < 0.01);
    }

    #[test]
    fn nq_rejects_wrong_units() {
        let expr = parse("nq(n, Q, H)").unwrap();
        let values = env(&[("n", "2900 m"), ("Q", "120 m3/h"), ("H", "45 m")]);
        assert!(eval(&expr, &values).is_err());
    }
}
