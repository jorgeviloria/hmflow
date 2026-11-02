//! Árbol de sintaxis de las fórmulas.

/// Operadores binarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

/// Operadores unarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
}

/// Expresión del DSL.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Var(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

impl Expr {
    pub fn binary(op: BinOp, left: Expr, right: Expr) -> Self {
        Self::Binary(op, Box::new(left), Box::new(right))
    }

    pub fn unary(op: UnOp, inner: Expr) -> Self {
        Self::Unary(op, Box::new(inner))
    }

    pub fn var(name: impl Into<String>) -> Self {
        Self::Var(name.into())
    }
}
