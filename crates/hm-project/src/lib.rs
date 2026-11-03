//! Proyecto de HidroMac Flow: formato `.hmf`.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

use std::collections::BTreeMap;
use std::path::Path;

pub use hm_core::PumpType;
use hm_core::{Diagnostic, Diagnostics};
use hm_formula::Registry;
use hm_formula::parse as parse_expression;
use hm_units::{parse_quantity, parse_unit};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Versión del esquema del proyecto.
///
/// - 1: proyecto, diseño, punto de operación y parámetros.
/// - 2: añade `viscosity` en `[operating]` y la sección opcional `[suction]`.
pub const SCHEMA_VERSION: u32 = 2;

/// Esquemas anteriores que se migran al cargar.
pub const MIGRATABLE_SCHEMAS: &[u32] = &[1];

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML inválido: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("no se pudo serializar: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("esquema no soportado: {0}")]
    Schema(u32),
}

/// Proyecto completo.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema: u32,
    pub project: ProjectMeta,
    pub design: Design,
    pub operating: Operating,
    /// Instalación de aspiración para calcular el NPSHa (esquema 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suction: Option<Suction>,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
}

/// Instalación de aspiración: NPSHa = (p − pv)/(ρg) + z − hf
/// (fórmula `npsh.npsha`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suction {
    /// Presión absoluta sobre la superficie del líquido aspirado.
    pub pressure: String,
    /// Altura del nivel del líquido sobre el eje de la bomba; negativa si
    /// la bomba aspira desde abajo. Vacía equivale a 0 m.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub height: String,
    /// Pérdidas de carga en la línea de aspiración. Vacía equivale a 0 m.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub losses: String,
    /// Presión de vapor del fluido. Vacía: la del agua a la temperatura del
    /// proyecto (IAPWS-IF97); obligatoria si el fluido no es agua.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vapor_pressure: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectMeta {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Design {
    pub pump_type: PumpType,
    #[serde(default = "default_stages")]
    pub stages: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub standard: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operating {
    #[serde(rename = "Q")]
    pub q: String,
    #[serde(rename = "H")]
    pub h: String,
    #[serde(rename = "n")]
    pub n: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fluid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub temperature: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub npsha: String,
    /// Viscosidad cinemática (m2/s, cSt) o dinámica (Pa*s, cP) del fluido.
    /// Vacía: la del agua a la temperatura del proyecto si el fluido es agua.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub viscosity: String,
}

/// Modo de un parámetro de diseño.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Param {
    Auto {
        formula: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
    },
    Manual {
        value: String,
    },
    Override {
        expr: String,
        author: String,
        date: String,
        reason: String,
    },
    Locked {
        value: String,
    },
}

fn default_stages() -> u32 {
    1
}

impl Project {
    pub fn new(name: &str, pump_type: PumpType) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            project: ProjectMeta {
                name: name.to_string(),
                notes: String::new(),
            },
            design: Design {
                pump_type,
                stages: 1,
                standard: pump_type.default_standard().to_string(),
                method: String::new(),
            },
            operating: Operating {
                q: "120 m3/h".to_string(),
                h: "45 m".to_string(),
                n: "2900 rpm".to_string(),
                fluid: "water".to_string(),
                temperature: "20 degC".to_string(),
                // El NPSHa depende de la instalación; no se inventa. Sin él,
                // el diseño avisa de que no evaluó la cavitación.
                npsha: String::new(),
                viscosity: String::new(),
            },
            suction: None,
            params: BTreeMap::new(),
        }
    }

    /// Lleva un proyecto de un esquema anterior al actual.
    ///
    /// 1 → 2: el esquema 2 solo añade campos opcionales, así que basta con
    /// cambiar el número. Un esquema desconocido se deja como está y lo
    /// señala `validate` con `HM-PROJ-001`.
    fn migrated(mut self) -> Self {
        if MIGRATABLE_SCHEMAS.contains(&self.schema) {
            self.schema = SCHEMA_VERSION;
        }
        self
    }

    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        let text = std::fs::read_to_string(path)?;
        let project: Project = toml::from_str(&text)?;
        Ok(project.migrated())
    }

    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        std::fs::write(path, self.to_toml()?)?;
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String, ProjectError> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn from_toml(text: &str) -> Result<Self, ProjectError> {
        let project: Project = toml::from_str(text)?;
        Ok(project.migrated())
    }

    /// Comprueba el proyecto y devuelve diagnósticos.
    pub fn validate(&self, registry: &Registry) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();

        if self.schema != SCHEMA_VERSION {
            diagnostics.push(
                Diagnostic::blocking(
                    "HM-PROJ-001",
                    format!(
                        "esquema {} no soportado; se esperaba {}",
                        self.schema, SCHEMA_VERSION
                    ),
                )
                .with_hint("Actualiza el proyecto o usa una versión compatible."),
            );
            return diagnostics;
        }

        if self.design.stages == 0 {
            diagnostics.push(
                Diagnostic::error("HM-PROJ-007", "el número de etapas debe ser al menos 1")
                    .with_hint("Un proyecto sin etapas no se puede calcular."),
            );
        }

        if self.design.pump_type == PumpType::EndSuction && self.design.stages != 1 {
            diagnostics.push(
                Diagnostic::error(
                    "HM-PROJ-024",
                    format!(
                        "el motor de diseño end-suction solo soporta una etapa; hay {}",
                        self.design.stages
                    ),
                )
                .with_hint("Usa un proyecto de una etapa o espera al diseño multietapa."),
            );
        }

        if self.design.pump_type != PumpType::EndSuction
            && self.design.standard.trim().eq_ignore_ascii_case("ISO 2858")
        {
            diagnostics.push(
                Diagnostic::error("HM-PROJ-022", "ISO 2858 es una norma de bombas end-suction")
                    .with_hint("Declara la norma correcta para este tipo o déjala vacía."),
            );
        }

        check_quantity(
            &mut diagnostics,
            "HM-PROJ-002",
            "Q",
            &self.operating.q,
            "m3/s",
            true,
        );
        check_quantity(
            &mut diagnostics,
            "HM-PROJ-003",
            "H",
            &self.operating.h,
            "m",
            true,
        );
        check_quantity(
            &mut diagnostics,
            "HM-PROJ-004",
            "n",
            &self.operating.n,
            "1/s",
            true,
        );

        if !self.operating.npsha.trim().is_empty() {
            check_quantity(
                &mut diagnostics,
                "HM-PROJ-005",
                "NPSHa",
                &self.operating.npsha,
                "m",
                false,
            );
        }

        if !self.operating.temperature.trim().is_empty() {
            check_quantity(
                &mut diagnostics,
                "HM-PROJ-006",
                "temperatura",
                &self.operating.temperature,
                "K",
                false,
            );
        }

        if !self.operating.viscosity.trim().is_empty() {
            check_viscosity(&mut diagnostics, &self.operating.viscosity);
        }

        if let Some(suction) = &self.suction {
            check_suction(&mut diagnostics, suction);
            if !self.operating.npsha.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        "HM-PROJ-028",
                        "el NPSHa está en [operating] y también se calcula con [suction]",
                    )
                    .with_hint("Deja solo uno de los dos."),
                );
            }
            let water = self.operating.fluid.is_empty()
                || self.operating.fluid.eq_ignore_ascii_case("water");
            if !water && suction.vapor_pressure.trim().is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        "HM-PROJ-029",
                        format!(
                            "el fluido es `{}` y [suction] no indica vapor_pressure",
                            self.operating.fluid
                        ),
                    )
                    .with_hint("Solo se conoce la presión de vapor del agua."),
                );
            }
        }

        for (name, param) in &self.params {
            match param {
                Param::Auto { formula, value } => {
                    if registry.get(formula).is_none() {
                        diagnostics.push(
                            Diagnostic::error(
                                "HM-PROJ-010",
                                format!("`{name}` usa una fórmula desconocida: {formula}"),
                            )
                            .with_hint("Revisa el id o actualiza el pack de fórmulas."),
                        );
                    }
                    if let Some(value) = value {
                        match parse_quantity(value) {
                            Ok(quantity) => {
                                if let Some(spec) = registry.get(formula)
                                    && let Ok(unit) = parse_unit(&spec.units)
                                    && quantity.dim() != unit.dim
                                {
                                    diagnostics.push(Diagnostic::error(
                                        "HM-PROJ-012",
                                        format!(
                                            "`{name}` no tiene la unidad de {formula}: {value}"
                                        ),
                                    ));
                                }
                            }
                            Err(error) => {
                                diagnostics.push(Diagnostic::error(
                                    "HM-PROJ-012",
                                    format!(
                                        "`{name}` tiene un valor calculado inválido: {value} ({error})"
                                    ),
                                ));
                            }
                        }
                    }
                }
                Param::Manual { value } | Param::Locked { value } => {
                    if let Err(error) = parse_quantity(value) {
                        diagnostics.push(Diagnostic::error(
                            "HM-PROJ-012",
                            format!("`{name}` tiene un valor inválido: {value} ({error})"),
                        ));
                    }
                }
                Param::Override {
                    expr,
                    author,
                    date,
                    reason,
                } => {
                    if author.trim().is_empty()
                        || date.trim().is_empty()
                        || reason.trim().is_empty()
                    {
                        diagnostics.push(
                            Diagnostic::error(
                                "HM-PROJ-011",
                                format!("`{name}` tiene un override sin autor, fecha o motivo"),
                            )
                            .with_hint("Un override debe poder auditarse."),
                        );
                    }
                    if let Err(error) = parse_expression(expr) {
                        diagnostics.push(Diagnostic::error(
                            "HM-PROJ-013",
                            format!("`{name}` tiene una expresión inválida: {error}"),
                        ));
                    }
                }
            }
        }

        diagnostics
    }
}

/// Viscosidad cinemática (m2/s) o dinámica (Pa*s), positiva.
fn check_viscosity(diagnostics: &mut Diagnostics, text: &str) {
    let problem = match parse_quantity(text) {
        Ok(quantity) => {
            let kinematic = parse_unit("m2/s").map(|unit| unit.dim);
            let dynamic = parse_unit("Pa*s").map(|unit| unit.dim);
            let dimension_ok = kinematic.is_ok_and(|dim| dim == quantity.dim())
                || dynamic.is_ok_and(|dim| dim == quantity.dim());
            if !dimension_ok {
                Some(format!("debe ser m2/s (cSt) o Pa*s (cP): {text}"))
            } else if !quantity.si.is_finite() || quantity.si <= 0.0 {
                Some(format!("debe ser positiva: {text}"))
            } else {
                None
            }
        }
        Err(error) => Some(format!("no se puede interpretar: {text} ({error})")),
    };
    if let Some(problem) = problem {
        diagnostics.push(Diagnostic::error(
            "HM-PROJ-026",
            format!("`viscosity` {problem}"),
        ));
    }
}

/// Campos de `[suction]`: presión positiva, altura cualquiera, pérdidas y
/// presión de vapor no negativas.
fn check_suction(diagnostics: &mut Diagnostics, suction: &Suction) {
    let fields = [
        ("pressure", suction.pressure.as_str(), "Pa", Some(true)),
        ("height", suction.height.as_str(), "m", None),
        ("losses", suction.losses.as_str(), "m", Some(false)),
        (
            "vapor_pressure",
            suction.vapor_pressure.as_str(),
            "Pa",
            Some(false),
        ),
    ];
    for (name, text, unit, sign) in fields {
        if text.trim().is_empty() && name != "pressure" {
            continue;
        }
        let problem = match (parse_quantity(text), parse_unit(unit)) {
            (Ok(quantity), Ok(expected)) => {
                if quantity.dim() != expected.dim {
                    Some(format!("debe ser {unit}"))
                } else if !quantity.si.is_finite() {
                    Some("debe ser finito".to_string())
                } else {
                    match sign {
                        Some(true) if quantity.si <= 0.0 => Some("debe ser positivo".to_string()),
                        Some(false) if quantity.si < 0.0 => {
                            Some("no puede ser negativo".to_string())
                        }
                        _ => None,
                    }
                }
            }
            (Err(error), _) | (_, Err(error)) => Some(format!("no se puede interpretar ({error})")),
        };
        if let Some(problem) = problem {
            diagnostics.push(Diagnostic::error(
                "HM-PROJ-027",
                format!("[suction] `{name}` {problem}: {text}"),
            ));
        }
    }
}

fn check_quantity(
    diagnostics: &mut Diagnostics,
    code: &str,
    label: &str,
    text: &str,
    expected_unit: &str,
    positive: bool,
) {
    let expected = match parse_unit(expected_unit) {
        Ok(unit) => unit,
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                code,
                format!("unidad esperada inválida para `{label}`: {error}"),
            ));
            return;
        }
    };

    match parse_quantity(text) {
        Ok(quantity) => {
            if !quantity.si.is_finite() {
                diagnostics.push(Diagnostic::error(
                    code,
                    format!("`{label}` debe ser finito: {text}"),
                ));
            } else if quantity.dim() != expected.dim {
                diagnostics.push(Diagnostic::error(
                    code,
                    format!("`{label}` debe ser {expected_unit}: {text}"),
                ));
            } else if positive && quantity.si <= 0.0 {
                diagnostics.push(Diagnostic::error(
                    code,
                    format!("`{label}` debe ser positivo: {text}"),
                ));
            }
        }
        Err(error) => {
            diagnostics.push(Diagnostic::error(
                code,
                format!("`{label}` no se puede interpretar: {text} ({error})"),
            ));
        }
    }
}
