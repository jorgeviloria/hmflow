//! Registro único de códigos de diagnóstico.
//!
//! Reglas (docs/adr/0003-versionado.md):
//!
//! - Un código tiene un solo significado y una sola severidad.
//! - Un código no se reutiliza nunca. Si deja de usarse, se marca como
//!   `Retired` y se queda en la tabla.
//! - El texto del mensaje puede cambiar; el código y su significado, no.
//!
//! `Diagnostic::new` comprueba en compilación de depuración que el código
//! está aquí y que la severidad coincide, y `cargo xtask codes` comprueba
//! que el código fuente y `docs/diagnostics.md` cuadran con la tabla.

use crate::Severity;

/// Estado de un código.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeStatus {
    /// Se emite.
    Active,
    /// Ya no se emite; el número queda reservado.
    Retired,
}

/// Un código de diagnóstico registrado.
#[derive(Debug, Clone, Copy)]
pub struct CodeInfo {
    pub code: &'static str,
    pub severity: Severity,
    pub status: CodeStatus,
    pub summary: &'static str,
}

const fn active(code: &'static str, severity: Severity, summary: &'static str) -> CodeInfo {
    CodeInfo {
        code,
        severity,
        status: CodeStatus::Active,
        summary,
    }
}

use Severity::{Blocking, Error, Info, Warning};

/// Todos los códigos, en orden.
pub const CODES: &[CodeInfo] = &[
    // Proyecto (.hmf)
    active(
        "HM-PROJ-001",
        Blocking,
        "Versión de esquema del proyecto no soportada.",
    ),
    active(
        "HM-PROJ-002",
        Error,
        "Caudal Q ilegible, con dimensión incorrecta o no positivo.",
    ),
    active(
        "HM-PROJ-003",
        Error,
        "Altura H ilegible, con dimensión incorrecta o no positiva.",
    ),
    active(
        "HM-PROJ-004",
        Error,
        "Velocidad n ilegible, con dimensión incorrecta o no positiva.",
    ),
    active(
        "HM-PROJ-005",
        Error,
        "NPSHa ilegible o con dimensión incorrecta.",
    ),
    active(
        "HM-PROJ-006",
        Error,
        "Temperatura ilegible o con dimensión incorrecta.",
    ),
    active("HM-PROJ-007", Error, "Número de etapas menor que 1."),
    active(
        "HM-PROJ-010",
        Error,
        "Parámetro auto con una fórmula que no existe.",
    ),
    active("HM-PROJ-011", Error, "Override sin autor, fecha o motivo."),
    active(
        "HM-PROJ-012",
        Error,
        "Valor guardado de un parámetro ilegible o con unidad incorrecta.",
    ),
    active("HM-PROJ-013", Error, "Expresión de un override inválida."),
    active(
        "HM-PROJ-014",
        Error,
        "No se pudo recalcular el diseño con los datos del proyecto.",
    ),
    active(
        "HM-PROJ-015",
        Error,
        "Valor auto guardado distinto del recalculado.",
    ),
    active(
        "HM-PROJ-016",
        Error,
        "Método de diseño desconocido o no aplicable al tipo de bomba.",
    ),
    active(
        "HM-PROJ-017",
        Error,
        "Falta un parámetro del método, o un auto sin valor guardado.",
    ),
    active(
        "HM-PROJ-018",
        Error,
        "Parámetro con un modo o una fórmula distintos de los del método.",
    ),
    active(
        "HM-PROJ-019",
        Error,
        "Selección del método en override: la auditoría no la evalúa.",
    ),
    active(
        "HM-PROJ-020",
        Error,
        "Valor de un parámetro del método inválido para su especificación.",
    ),
    active(
        "HM-PROJ-021",
        Info,
        "Tipo de bomba sin motor de diseño: solo se validó el formato.",
    ),
    active(
        "HM-PROJ-022",
        Error,
        "ISO 2858 declarada para un tipo de bomba que no es end-suction.",
    ),
    active(
        "HM-PROJ-023",
        Error,
        "Parámetro desconocido para el método.",
    ),
    active("HM-PROJ-024", Error, "End-suction con más de una etapa."),
    active(
        "HM-PROJ-025",
        Error,
        "Proyecto de un método anterior: hay que migrarlo con recalc.",
    ),
    active(
        "HM-PROJ-026",
        Error,
        "Viscosidad ilegible, con dimensión incorrecta o no positiva.",
    ),
    active(
        "HM-PROJ-027",
        Error,
        "Dato de [suction] ilegible, con dimensión incorrecta o fuera de rango.",
    ),
    active(
        "HM-PROJ-028",
        Error,
        "NPSHa dado a la vez en [operating] y en [suction].",
    ),
    active(
        "HM-PROJ-029",
        Error,
        "Falta la presión de vapor de [suction] para un fluido que no es agua.",
    ),
    // Diseño
    active(
        "HM-DES-001",
        Warning,
        "nq fuera del rango calibrado 10..100.",
    ),
    active(
        "HM-DES-002",
        Warning,
        "Velocidad periférica u2 por encima de 60 m/s.",
    ),
    active(
        "HM-DES-003",
        Warning,
        "b2/d2 lejos de la media de Gülich (7.1).",
    ),
    active("HM-DES-004", Warning, "β2 fuera de 15..45°."),
    active("HM-DES-005", Warning, "Número de álabes fuera de 4..12."),
    active(
        "HM-DES-006",
        Info,
        "NPSHa suficiente en la región preferente; informa del margen.",
    ),
    active(
        "HM-DES-007",
        Error,
        "NPSHa insuficiente en la región preferente (Gülich, tabla 6.2).",
    ),
    active(
        "HM-DES-008",
        Warning,
        "Componente tangencial cu2 no positiva.",
    ),
    active(
        "HM-DES-009",
        Warning,
        "Densidad distinta de la del agua a la temperatura indicada.",
    ),
    active(
        "HM-DES-010",
        Warning,
        "Agua fuera del intervalo de la correlación de densidad.",
    ),
    active(
        "HM-DES-011",
        Warning,
        "NPSHa insuficiente fuera de la región preferente.",
    ),
    active(
        "HM-DES-012",
        Warning,
        "NPSHa no indicado: no se evaluó la cavitación.",
    ),
    active(
        "HM-DES-013",
        Warning,
        "Selecciones manuales que no corresponden al nq actual.",
    ),
    active(
        "HM-DES-014",
        Warning,
        "w2/w1 por debajo de 0,7 (de Haller).",
    ),
    active(
        "HM-DES-015",
        Warning,
        "u1 por encima de 50 m/s: solo se aplicó el margen mínimo de NPSH.",
    ),
    active("HM-DES-016", Error, "H0 por debajo de la altura de diseño."),
    active("HM-DES-017", Info, "H0/Hd por debajo de 1,2 con nq < 40."),
    active(
        "HM-DES-018",
        Warning,
        "El NPSHA necesario supone agua y el fluido es otro.",
    ),
    active(
        "HM-DES-019",
        Warning,
        "Con β2 manual, la altura de Euler difiere más de un 3 %.",
    ),
    active(
        "HM-DES-020",
        Warning,
        "Con la viscosidad del fluido, la bomba no cumple el punto pedido (Gülich, tabla 13.1).",
    ),
    active(
        "HM-DES-021",
        Warning,
        "Corrección por viscosidad fuera de su rango de validez.",
    ),
    active(
        "HM-DES-022",
        Info,
        "Fluido distinto de agua sin viscosidad: no se evaluó su efecto.",
    ),
    active(
        "HM-DES-023",
        Warning,
        "Lengua de la voluta más cerca del rodete que el mínimo de Gülich (tabla 10.2).",
    ),
    active(
        "HM-DES-024",
        Info,
        "Con nq > 40 el álabe cilíndrico de la geometría es una simplificación.",
    ),
    active(
        "HM-DES-025",
        Warning,
        "La subida brusca del NPSH3 (Qsa) empieza antes del punto de diseño.",
    ),
    active(
        "HM-DES-026",
        Warning,
        "La recirculación a la entrada puede empezar dentro de la región preferente.",
    ),
];

/// Busca un código en el registro.
pub fn code_info(code: &str) -> Option<&'static CodeInfo> {
    CODES.iter().find(|info| info.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for info in CODES {
            assert!(seen.insert(info.code), "código repetido: {}", info.code);
            let parts: Vec<&str> = info.code.split('-').collect();
            assert_eq!(parts.len(), 3, "{}", info.code);
            assert_eq!(parts[0], "HM", "{}", info.code);
            assert!(
                parts[1].chars().all(|c| c.is_ascii_uppercase()),
                "{}",
                info.code
            );
            assert!(
                parts[2].len() == 3 && parts[2].chars().all(|c| c.is_ascii_digit()),
                "{}",
                info.code
            );
            assert!(!info.summary.trim().is_empty(), "{}", info.code);
        }
    }

    #[test]
    fn lookup_finds_registered_codes() {
        assert_eq!(code_info("HM-DES-007").map(|i| i.severity), Some(Error));
        assert!(code_info("HM-XXX-999").is_none());
    }
}
