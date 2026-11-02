use thiserror::Error;

/// Errores del motor de fórmulas.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum FormulaError {
    #[error("error de sintaxis en la posición {position}: {message}")]
    Syntax { position: usize, message: String },

    #[error("variable desconocida: {0}")]
    UnknownVariable(String),

    #[error("función desconocida: {0}")]
    UnknownFunction(String),

    #[error("argumentos incorrectos en {function}: se esperaban {expected}, llegaron {got}")]
    Arity {
        function: String,
        expected: usize,
        got: usize,
    },

    #[error("dimensión incompatible en {context}: {left} frente a {right}")]
    DimensionMismatch {
        context: String,
        left: String,
        right: String,
    },

    #[error("el exponente debe ser constante")]
    NonConstantExponent,

    #[error("el resultado no es una magnitud física: {0}")]
    NonPhysical(String),

    #[error("unidad inválida: {0}")]
    Unit(String),

    #[error("la fórmula no declara la expresión `value`")]
    MissingValue,

    #[error("entrada requerida ausente: {0}")]
    MissingInput(String),

    #[error("entrada no declarada: {0}")]
    UnexpectedInput(String),

    #[error("fórmula desconocida: {0}")]
    UnknownFormula(String),

    #[error("pack inválido: {0}")]
    Pack(String),

    #[error("división por cero")]
    DivisionByZero,

    #[error("error de unidades: {0}")]
    Units(#[from] hm_units::UnitError),
}
