use thiserror::Error;

/// Errores de unidades y dimensiones.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum UnitError {
    #[error("unidad desconocida: {0}")]
    UnknownUnit(String),

    #[error("expresión de unidades inválida: {0}")]
    InvalidUnit(String),

    #[error("no se pudo interpretar la cantidad: {0}")]
    InvalidQuantity(String),

    #[error("dimensión incompatible: {left} frente a {right}")]
    DimensionMismatch { left: String, right: String },

    #[error("división por cero")]
    DivisionByZero,

    #[error("el resultado no es una magnitud física: {0}")]
    NonPhysical(String),
}
