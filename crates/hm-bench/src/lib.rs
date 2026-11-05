//! Casos de ensayo de banco: lectura, reducción y comparación con el modelo.
//!
//! El caso guarda lo medido; todo lo demás se recalcula aquí. El formato está
//! en `docs/validation/caso-de-banco.md`.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

mod case;
mod compare;
mod reduce;

pub use case::{
    BenchCase, BenchError, Level, NpshReading, Reading, Rig, Specified, Uncertainties, Uncertainty,
};
pub use compare::{Comparison, NpshComparison, PointComparison, compare};
pub use reduce::{NpshSeries, Point, PointUncertainty, Reduction, reduce};
