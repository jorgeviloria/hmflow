//! Proyecto de HidroMac Flow: formato `.hmf`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use hm_core::{Diagnostic, Diagnostics};
use hm_formula::Registry;
use hm_formula::parse as parse_expression;
use hm_units::{parse_quantity, parse_unit};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Versión del esquema del proyecto.
pub const SCHEMA_VERSION: u32 = 1;

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
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpType {
    EndSuction,
    Inline,
    RingSection,
    SplitCase,
    Barrel,
}

impl PumpType {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "end_suction" | "end-suction" | "oh1" | "oh2" => Some(Self::EndSuction),
            "inline" | "oh3" | "oh4" => Some(Self::Inline),
            "ring_section" | "ring-section" | "bb4" => Some(Self::RingSection),
            "split_case" | "split-case" | "bb1" | "bb2" | "bb3" => Some(Self::SplitCase),
            "barrel" | "bb5" => Some(Self::Barrel),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::EndSuction => "end_suction",
            Self::Inline => "inline",
            Self::RingSection => "ring_section",
            Self::SplitCase => "split_case",
            Self::Barrel => "barrel",
        }
    }

    /// Norma dimensional por defecto. Solo end-suction tiene una asignada;
    /// el resto la declara el usuario.
    pub fn default_standard(self) -> &'static str {
        match self {
            Self::EndSuction => "ISO 2858",
            Self::Inline | Self::RingSection | Self::SplitCase | Self::Barrel => "",
        }
    }
}

impl fmt::Display for PumpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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
                // 10 m deja al menos 0,5 m sobre el NPSHr de la curva
                // preliminar en el punto de ejemplo (120 m3/h, 45 m, 2900 rpm).
                npsha: "10 m".to_string(),
            },
            params: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        let text = std::fs::read_to_string(path)?;
        let project: Project = toml::from_str(&text)?;
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        std::fs::write(path, self.to_toml()?)?;
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String, ProjectError> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn from_toml(text: &str) -> Result<Self, ProjectError> {
        Ok(toml::from_str(text)?)
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
                    "HM-PROJ-021",
                    format!(
                        "el motor de diseño end-suction v0.1 solo soporta una etapa; hay {}",
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
