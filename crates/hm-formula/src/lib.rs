//! Motor de fórmulas de HidroMac Flow.
//!
//! Una fórmula es un TOML con entradas tipadas, una expresión, una fuente
//! pública, un rango de validez y un test. El motor comprueba dimensiones antes
//! de evaluar y nunca devuelve un `NaN` silencioso.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

mod ast;
mod error;
mod eval;
mod lexer;
mod parser;
mod registry;

pub use ast::{BinOp, Expr, UnOp};
pub use error::FormulaError;
pub use eval::{constant, eval, infer};
pub use parser::parse;
pub use registry::{AuditIssue, FormulaSpec, Notes, Pack, Registry, Validation};

/// Pack de fórmulas integrado.
pub const BUILTIN_PACK: &str = include_str!("../packs/pilot.toml");

/// Pack de diseño preliminar para bombas end-suction OH1/OH2.
pub const END_SUCTION_V01_PACK: &str = include_str!("../packs/end_suction_v01.toml");
