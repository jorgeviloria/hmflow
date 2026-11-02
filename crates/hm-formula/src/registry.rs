//! Catálogo de fórmulas y su auditoría.

use std::collections::BTreeMap;
use std::fmt;

use hm_units::{Dimension, Quantity, parse_unit};
use serde::Deserialize;

use crate::error::FormulaError;
use crate::eval::{eval, infer};
use crate::parser::parse;

/// Un pack de fórmulas en TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct Pack {
    pub formula: Vec<FormulaSpec>,
}

/// Especificación completa de una fórmula.
#[derive(Debug, Clone, Deserialize)]
pub struct FormulaSpec {
    pub id: String,
    pub version: u32,
    pub module: String,
    pub title: String,
    pub source: String,
    pub units: String,
    pub range: String,
    pub test: String,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    pub expr: BTreeMap<String, String>,
    #[serde(default)]
    pub notes: Option<Notes>,
}

/// Notas y supuestos de una fórmula.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Notes {
    #[serde(default)]
    pub assumptions: Option<String>,
}

impl FormulaSpec {
    /// Evalúa la fórmula con las entradas dadas y devuelve el resultado en su
    /// unidad de salida.
    pub fn evaluate(&self, inputs: &BTreeMap<String, Quantity>) -> Result<Quantity, FormulaError> {
        let mut environment = BTreeMap::new();

        for (name, unit_text) in &self.inputs {
            let unit = parse_unit(unit_text)
                .map_err(|error| FormulaError::Unit(format!("{name}: {error}")))?;
            let reference = Quantity::new(1.0, unit);
            let value = inputs
                .get(name)
                .ok_or_else(|| FormulaError::MissingInput(name.clone()))?;
            value.require_same(&reference)?;
            environment.insert(name.clone(), value.clone());
        }

        for name in inputs.keys() {
            if !self.inputs.contains_key(name) {
                return Err(FormulaError::UnexpectedInput(name.clone()));
            }
        }

        let expression_text = self.expr.get("value").ok_or(FormulaError::MissingValue)?;
        let expression = parse(expression_text)?;

        let dimensions: BTreeMap<String, Dimension> = environment
            .iter()
            .map(|(name, value)| (name.clone(), value.dim()))
            .collect();

        let inferred = infer(&expression, &dimensions)?;
        let output_unit = parse_unit(&self.units)
            .map_err(|error| FormulaError::Unit(format!("salida: {error}")))?;

        if inferred != output_unit.dim {
            return Err(FormulaError::DimensionMismatch {
                context: format!("salida de {}", self.id),
                left: inferred.symbol(),
                right: output_unit.dim.symbol(),
            });
        }

        let result = eval(&expression, &environment)?;
        let converted = result.to(&output_unit)?;
        converted.ensure_physical()?;
        Ok(converted)
    }
}

/// Problema detectado al auditar un pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditIssue {
    MissingSource(String),
    MissingRange(String),
    MissingTest(String),
    MissingValue(String),
    BadInput {
        formula: String,
        input: String,
        message: String,
    },
    BadExpression {
        formula: String,
        message: String,
    },
    BadOutputUnit {
        formula: String,
        message: String,
    },
    DimensionMismatch {
        formula: String,
        left: String,
        right: String,
    },
}

impl fmt::Display for AuditIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSource(id) => write!(f, "{id}: falta `source`"),
            Self::MissingRange(id) => write!(f, "{id}: falta `range`"),
            Self::MissingTest(id) => write!(f, "{id}: falta `test`"),
            Self::MissingValue(id) => write!(f, "{id}: falta la expresión `value`"),
            Self::BadInput {
                formula,
                input,
                message,
            } => write!(f, "{formula}: entrada `{input}` inválida: {message}"),
            Self::BadExpression { formula, message } => {
                write!(f, "{formula}: expresión inválida: {message}")
            }
            Self::BadOutputUnit { formula, message } => {
                write!(f, "{formula}: unidad de salida inválida: {message}")
            }
            Self::DimensionMismatch {
                formula,
                left,
                right,
            } => write!(f, "{formula}: la salida es {left}, se declaró {right}"),
        }
    }
}

/// Catálogo de fórmulas indexado por id.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    specs: BTreeMap<String, FormulaSpec>,
}

impl Registry {
    pub fn from_toml(text: &str) -> Result<Self, FormulaError> {
        let pack: Pack =
            toml::from_str(text).map_err(|error| FormulaError::Pack(error.to_string()))?;
        let mut specs = BTreeMap::new();

        for spec in pack.formula {
            if specs.contains_key(&spec.id) {
                return Err(FormulaError::Pack(format!("id duplicado: {}", spec.id)));
            }
            specs.insert(spec.id.clone(), spec);
        }

        Ok(Self { specs })
    }

    pub fn builtin() -> Self {
        Self::from_toml(crate::BUILTIN_PACK).expect("el pack integrado debe ser válido")
    }

    pub fn get(&self, id: &str) -> Option<&FormulaSpec> {
        self.specs.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &FormulaSpec> {
        self.specs.values()
    }

    pub fn len(&self) -> usize {
        self.specs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    pub fn evaluate(
        &self,
        id: &str,
        inputs: &BTreeMap<String, Quantity>,
    ) -> Result<Quantity, FormulaError> {
        let spec = self
            .get(id)
            .ok_or_else(|| FormulaError::UnknownFormula(id.to_string()))?;
        spec.evaluate(inputs)
    }

    /// Comprueba que cada fórmula tenga fuente, rango, test y dimensiones
    /// coherentes.
    pub fn audit(&self) -> Vec<AuditIssue> {
        let mut issues = Vec::new();

        for spec in self.specs.values() {
            if spec.source.trim().is_empty() {
                issues.push(AuditIssue::MissingSource(spec.id.clone()));
            }
            if spec.range.trim().is_empty() {
                issues.push(AuditIssue::MissingRange(spec.id.clone()));
            }
            if spec.test.trim().is_empty() {
                issues.push(AuditIssue::MissingTest(spec.id.clone()));
            }

            let mut dimensions = BTreeMap::new();
            let mut inputs_ok = true;

            for (name, unit_text) in &spec.inputs {
                match parse_unit(unit_text) {
                    Ok(unit) => {
                        dimensions.insert(name.clone(), unit.dim);
                    }
                    Err(error) => {
                        issues.push(AuditIssue::BadInput {
                            formula: spec.id.clone(),
                            input: name.clone(),
                            message: error.to_string(),
                        });
                        inputs_ok = false;
                    }
                }
            }

            if !inputs_ok {
                continue;
            }

            let Some(expression_text) = spec.expr.get("value") else {
                issues.push(AuditIssue::MissingValue(spec.id.clone()));
                continue;
            };

            let expression = match parse(expression_text) {
                Ok(expression) => expression,
                Err(error) => {
                    issues.push(AuditIssue::BadExpression {
                        formula: spec.id.clone(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };

            let inferred = match infer(&expression, &dimensions) {
                Ok(dimension) => dimension,
                Err(error) => {
                    issues.push(AuditIssue::BadExpression {
                        formula: spec.id.clone(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };

            match parse_unit(&spec.units) {
                Ok(output) => {
                    if inferred != output.dim {
                        issues.push(AuditIssue::DimensionMismatch {
                            formula: spec.id.clone(),
                            left: inferred.symbol(),
                            right: output.dim.symbol(),
                        });
                    }
                }
                Err(error) => issues.push(AuditIssue::BadOutputUnit {
                    formula: spec.id.clone(),
                    message: error.to_string(),
                }),
            }
        }

        issues
    }
}
