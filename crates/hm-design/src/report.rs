//! Informe serializable del diseño: el contrato con la interfaz y el CAD.
//!
//! Los consumidores leen este informe, no `DesignResult`. Reglas
//! (docs/adr/0003-versionado.md y docs/report-format.md):
//!
//! - Todos los valores van en SI, cada uno con su unidad: `{value, unit}`.
//! - Los nombres de `results` son los de los parámetros del `.hmf`.
//! - Añadir campos no cambia `version`; quitar, renombrar o cambiar el
//!   significado de uno sí. Los consumidores deben ignorar campos que no
//!   conocen.

use std::collections::BTreeMap;

use hm_core::Diagnostic;
use serde::{Deserialize, Serialize};

use crate::{
    DesignInput, DesignResult, METHOD, PARAM_SPECS, ViscousCorrection, designed_si,
    performance_curve,
};

/// Identificador del formato del informe.
pub const REPORT_FORMAT: &str = "hmflow.design-report";

/// Versión del formato del informe.
pub const REPORT_VERSION: u32 = 1;

/// Un valor en SI con su unidad.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measure {
    pub value: f64,
    pub unit: String,
}

impl Measure {
    fn new(value: f64, unit: &str) -> Self {
        Self {
            value,
            unit: unit.to_string(),
        }
    }
}

/// Sección de la voluta en un ángulo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportSection {
    pub angle: Measure,
    pub area: Measure,
    pub diameter: Measure,
    pub outer_radius: Measure,
}

/// Punto de la curva preliminar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportPoint {
    pub q: Measure,
    pub h: Measure,
    pub eta: Measure,
    pub p_shaft: Measure,
    pub npshr: Measure,
}

/// Prestaciones con el fluido viscoso en el punto de diseño
/// (Gülich 2020, tabla 13.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportViscous {
    pub nu: Measure,
    pub reynolds_mod: Measure,
    pub f_q: Measure,
    pub f_h: Measure,
    pub f_eta: Measure,
    pub flow: Measure,
    pub head: Measure,
    pub efficiency: Measure,
    pub shaft_power: Measure,
}

/// Informe completo de un diseño.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesignReport {
    /// Siempre [`REPORT_FORMAT`].
    pub format: String,
    /// [`REPORT_VERSION`] con que se escribió.
    pub version: u32,
    /// Método de diseño, [`METHOD`].
    pub method: String,
    /// Punto de operación y datos del fluido.
    pub input: BTreeMap<String, Measure>,
    /// Selecciones usadas (por defecto o manuales).
    pub selection: BTreeMap<String, Measure>,
    /// Valores calculados, con los nombres de los parámetros `auto` del `.hmf`
    /// y algunos más (`u1`, `w1`, `w2`, `torque_max`).
    pub results: BTreeMap<String, Measure>,
    pub volute_sections: Vec<ReportSection>,
    pub curve: Vec<ReportPoint>,
    /// Corrección por viscosidad; ausente si no se evaluó.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viscous: Option<ReportViscous>,
    pub diagnostics: Vec<Diagnostic>,
}

impl DesignReport {
    /// Construye el informe de `result`, con `curve_points` puntos de curva y
    /// los diagnósticos que se le pasen (los del diseño y los de la auditoría
    /// del proyecto, si los hay).
    pub fn new(
        input: &DesignInput,
        result: &DesignResult,
        curve_points: usize,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        let mut input_map = BTreeMap::new();
        input_map.insert("Q".to_string(), Measure::new(input.q.si, "m3/s"));
        input_map.insert("H".to_string(), Measure::new(input.h.si, "m"));
        input_map.insert("n".to_string(), Measure::new(input.n.si, "1/s"));
        input_map.insert("rho".to_string(), Measure::new(input.rho.si, "kg/m3"));
        input_map.insert("eta_m".to_string(), Measure::new(input.eta_m, "1"));
        if let Some(npsha) = &input.npsha {
            input_map.insert("npsha".to_string(), Measure::new(npsha.si, "m"));
        }

        let selection = &result.selection;
        let mut selection_map = BTreeMap::new();
        for (name, value, unit) in [
            ("psi", selection.psi, "1"),
            ("phi", selection.phi, "1"),
            ("beta2", selection.beta2_deg.to_radians(), "rad"),
            ("blade_count", f64::from(selection.blade_count), "1"),
            ("c0", selection.c0, "m/s"),
            ("cm0", selection.cm0, "m/s"),
            ("A", selection.a, "1"),
            ("J", selection.j, "1"),
            ("margin", selection.margin, "1"),
        ] {
            selection_map.insert(name.to_string(), Measure::new(value, unit));
        }

        let mut results = BTreeMap::new();
        for spec in PARAM_SPECS.iter().filter(|spec| spec.formula.is_some()) {
            if let Some(value) = designed_si(result, spec.name) {
                results.insert(spec.name.to_string(), Measure::new(value, spec.unit));
            }
        }
        for (name, value, unit) in [
            ("u1", result.u1.si, "m/s"),
            ("w1", result.w1.si, "m/s"),
            ("w2", result.w2.si, "m/s"),
            ("torque_max", result.torque_max.si, "N*m"),
        ] {
            results.insert(name.to_string(), Measure::new(value, unit));
        }

        let volute_sections = result
            .volute
            .sections
            .iter()
            .map(|section| ReportSection {
                angle: Measure::new(section.angle_deg.to_radians(), "rad"),
                area: Measure::new(section.area.si, "m2"),
                diameter: Measure::new(section.diameter.si, "m"),
                outer_radius: Measure::new(section.outer_radius.si, "m"),
            })
            .collect();

        let curve = performance_curve(input, result, curve_points)
            .into_iter()
            .map(|point| ReportPoint {
                q: Measure::new(point.q.si, "m3/s"),
                h: Measure::new(point.h.si, "m"),
                eta: Measure::new(point.eta, "1"),
                p_shaft: Measure::new(point.p_shaft.si, "W"),
                npshr: Measure::new(point.npshr.si, "m"),
            })
            .collect();

        Self {
            format: REPORT_FORMAT.to_string(),
            version: REPORT_VERSION,
            method: METHOD.to_string(),
            input: input_map,
            selection: selection_map,
            results,
            volute_sections,
            curve,
            viscous: None,
            diagnostics,
        }
    }

    /// Añade la corrección por viscosidad.
    pub fn with_viscous(mut self, correction: &ViscousCorrection) -> Self {
        self.viscous = Some(ReportViscous {
            nu: Measure::new(correction.nu, "m2/s"),
            reynolds_mod: Measure::new(correction.reynolds_mod, "1"),
            f_q: Measure::new(correction.f_q, "1"),
            f_h: Measure::new(correction.f_h, "1"),
            f_eta: Measure::new(correction.f_eta, "1"),
            flow: Measure::new(correction.flow.si, "m3/s"),
            head: Measure::new(correction.head.si, "m"),
            efficiency: Measure::new(correction.efficiency, "1"),
            shaft_power: Measure::new(correction.shaft_power.si, "W"),
        });
        self
    }
}
