//! Cambios que `recalc` informa al reescribir un proyecto.
//!
//! El orden del vector es el de la impresión de la CLI: selecciones vueltas
//! a tomar, manuales rellenados, ηm de un método que lo lumpaba con los
//! discos, y al final la migración del método.

/// Cómo cambió un parámetro o el método al recalcular.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Faltaba y se rellenó con el valor del motor.
    Filled,
    /// Una selección manual se volvió a tomar de la tabla del nq.
    Reselected,
    /// El método anterior pasó al método actual.
    MethodMigrated,
    /// ηm de v0.1–v0.3 incluía el rozamiento de discos.
    MechanicalEfficiency,
}

/// Un cambio que la CLI imprime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamChange {
    pub name: String,
    /// Texto anterior. `None` si el parámetro no estaba.
    pub before: Option<String>,
    pub after: String,
    pub kind: ChangeKind,
}
