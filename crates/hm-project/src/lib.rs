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
/// - 3: añade la sección opcional `[casing]`.
/// - 4: añade la sección opcional `[meridian]`, con P2 y las fracciones de P1
///   y P3 de cada tapa.
pub const SCHEMA_VERSION: u32 = 4;

/// Esquemas anteriores que se migran al cargar.
pub const MIGRATABLE_SCHEMAS: &[u32] = &[1, 2, 3];

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
    /// Carcasa existente para la que se diseña el rodete (esquema 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub casing: Option<Casing>,
    /// Brazos editados del corte meridional (esquema 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meridian: Option<Meridian>,
    #[serde(default)]
    pub params: BTreeMap<String, Param>,
}

/// Lo editado del corte meridional: cada tapa es una Bézier de 5 puntos. Lo
/// que falta es lo del motor. Si algo no cabe en el corte, lo dice el motor
/// al trazarlo.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meridian {
    /// Tapa delantera.
    #[serde(default, skip_serializing_if = "MeridianWall::is_empty")]
    pub shroud: MeridianWall,
    /// Tapa trasera (cubo).
    #[serde(default, skip_serializing_if = "MeridianWall::is_empty")]
    pub hub: MeridianWall,
}

/// Lo editado de una tapa, como texto.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeridianWall {
    /// P2: z (con el flujo, 0 en el borde de ataque) y r, con unidad. Van
    /// los dos o ninguno.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub p2_z: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub p2_r: String,
    /// Fracción de P1 sobre P0–P2, entre 0 y 1.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub p1: String,
    /// Fracción de P3 sobre P4–P2, entre 0 y 1.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub p3: String,
}

impl MeridianWall {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [&self.p2_z, &self.p2_r, &self.p1, &self.p3]
            .iter()
            .all(|text| text.trim().is_empty())
    }
}

impl Meridian {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shroud.is_empty() && self.hub.is_empty()
    }
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

/// Lo que una carcasa existente le impone al rodete. Con esta sección, el
/// diseño deja de elegir estas cotas: son datos.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Casing {
    /// Diámetro exterior del rodete.
    pub d2: String,
    /// Cuello del anillo de desgaste delantero.
    pub neck_diameter: String,
    /// Mayor ojo que deja el cuello, con su pared.
    pub eye_diameter_max: String,
    /// Agujero del eje.
    pub bore_diameter: String,
    /// Cubo mínimo en el borde de ataque.
    pub hub_diameter: String,
    /// Ancho de entrada de la voluta.
    pub b3: String,
    /// Radio de la lengua.
    pub cutwater_radius: String,
    /// Canales de la voluta: 1 o 2.
    pub channels: u32,
    /// Separación entre las dos lenguas (Gülich sec. 9.3.4). Solo con 2 canales.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wrap: String,
    /// Área de garganta de cada canal. Vacía: sin dato, y no se inventa.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub throat_area: String,
    /// Holgura diametral de los dos anillos. Con los otros datos de fuga.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub seal_clearance: String,
    /// Largo del anillo delantero.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub seal_length: String,
    /// Diámetro de sellado del anillo trasero.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rear_seal_diameter: String,
    /// Largo del anillo trasero.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rear_seal_length: String,
    /// Agujeros de equilibrio en la tapa trasera.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub balance_hole_count: u32,
    /// Diámetro de cada agujero de equilibrio.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub balance_hole_diameter: String,
    /// Diámetro del círculo de los agujeros de equilibrio.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub balance_hole_circle: String,
    /// Diámetro del cierre del eje. Opcional: sin él, el empuje usa el cubo.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shaft_seal_diameter: String,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Un archivo que solo trae la tabla `[casing]`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CasingFile {
    casing: Casing,
}

impl Casing {
    /// Si declara algún dato de fuga. Van todos o ninguno: lo comprueba `validate`.
    pub fn has_leakage(&self) -> bool {
        self.balance_hole_count > 0
            || [
                &self.seal_clearance,
                &self.seal_length,
                &self.rear_seal_diameter,
                &self.rear_seal_length,
                &self.balance_hole_diameter,
                &self.balance_hole_circle,
                &self.shaft_seal_diameter,
            ]
            .iter()
            .any(|text| !text.trim().is_empty())
    }

    /// Lee un archivo con una tabla `[casing]`.
    pub fn from_toml(text: &str) -> Result<Self, ProjectError> {
        Ok(toml::from_str::<CasingFile>(text)?.casing)
    }
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

/// Escribe en un temporal junto a `path` y lo pone en su lugar con un rename.
///
/// `fs::write` vacía el archivo antes de escribir: un disco lleno se llevaría
/// el proyecto. El temporal va en la misma carpeta porque el rename solo es
/// atómico dentro de un mismo sistema de archivos.
///
/// El rename no mira los permisos del destino ni sigue enlaces, así que eso se
/// resuelve aquí: un enlace escribe en el archivo real, un archivo de solo
/// lectura es un error y los permisos se conservan.
fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let existing = std::fs::metadata(&target).ok();
    if existing
        .as_ref()
        .is_some_and(|metadata| metadata.permissions().readonly())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "el archivo es de solo lectura",
        ));
    }
    let Some(name) = target.file_name() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "la ruta no tiene nombre de archivo",
        ));
    };
    let mut temporary = std::ffi::OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".tmp-{}", std::process::id()));
    let temporary = target.with_file_name(temporary);

    let written = write_durably(&temporary, bytes)
        .and_then(|()| match &existing {
            Some(metadata) => std::fs::set_permissions(&temporary, metadata.permissions()),
            None => Ok(()),
        })
        .and_then(|()| std::fs::rename(&temporary, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

fn write_durably(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.sync_all()
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
            casing: None,
            meridian: None,
            params: BTreeMap::new(),
        }
    }

    /// Lleva un proyecto de un esquema anterior al actual.
    ///
    /// Los esquemas 2, 3 y 4 solo añaden campos opcionales, así que basta con
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

    /// Escribe el proyecto. Si algo falla, un archivo que ya existía queda igual.
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        replace_file(path, self.to_toml()?.as_bytes())?;
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

        if let Some(casing) = &self.casing {
            check_casing(&mut diagnostics, casing);
        }

        if let Some(meridian) = &self.meridian {
            check_meridian(&mut diagnostics, meridian);
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

/// Envolvente admitida en `[casing]`, en grados: la misma de `volute_wrap_deg`.
const CASING_WRAP_RANGE_DEG: (f64, f64) = (90.0, 180.0);

fn casing_problem(diagnostics: &mut Diagnostics, name: &str, problem: &str) {
    diagnostics.push(
        Diagnostic::error("HM-PROJ-030", format!("[casing] `{name}` {problem}"))
            .with_hint("Corrige la cota con la del plano de la carcasa."),
    );
}

/// Valor en SI, positivo y finito, o el diagnóstico que nombra el campo.
fn casing_value(diagnostics: &mut Diagnostics, name: &str, text: &str, unit: &str) -> Option<f64> {
    match positive_value(text, unit) {
        Ok(value) => Some(value),
        Err(problem) => {
            casing_problem(diagnostics, name, &problem);
            None
        }
    }
}

/// Cada campo de `[meridian]`: P2 como longitudes (z cualquiera, r
/// positiva), los dos o ninguno, y las fracciones entre 0 y 1.
fn check_meridian(diagnostics: &mut Diagnostics, meridian: &Meridian) {
    let mut problem = |name: String, text: String| {
        diagnostics.push(
            Diagnostic::error("HM-PROJ-031", format!("[meridian] `{name}` {text}"))
                .with_hint("P2 lleva z y r con unidad, como \"24 mm\"; P1 y P3, una fracción entre 0 y 1. Vacío: el motor."),
        );
    };
    for (table, wall) in [("shroud", &meridian.shroud), ("hub", &meridian.hub)] {
        let (z, r) = (wall.p2_z.trim(), wall.p2_r.trim());
        if z.is_empty() != r.is_empty() {
            problem(
                format!("{table}.p2"),
                "necesita z y r, o ninguno".to_string(),
            );
        }
        if !z.is_empty()
            && let Err(text) = length_value(z)
        {
            problem(format!("{table}.p2_z"), text);
        }
        if !r.is_empty()
            && let Err(text) = positive_value(r, "m")
        {
            problem(format!("{table}.p2_r"), text);
        }
        for (name, text) in [("p1", wall.p1.trim()), ("p3", wall.p3.trim())] {
            if text.is_empty() {
                continue;
            }
            match text.parse::<f64>() {
                Ok(value) if value > 0.0 && value < 1.0 => {}
                _ => problem(
                    format!("{table}.{name}"),
                    format!("debe estar entre 0 y 1: {text}"),
                ),
            }
        }
    }
}

/// Una longitud finita, de cualquier signo.
fn length_value(text: &str) -> Result<f64, String> {
    match (parse_quantity(text), parse_unit("m")) {
        (Ok(quantity), Ok(expected)) if quantity.dim() != expected.dim => {
            Err(format!("debe ser una longitud: {text}"))
        }
        (Ok(quantity), Ok(_)) if !quantity.si.is_finite() => {
            Err(format!("debe ser finito: {text}"))
        }
        (Ok(quantity), Ok(_)) => Ok(quantity.si),
        (Err(error), _) | (_, Err(error)) => {
            Err(format!("no se puede interpretar ({error}): {text}"))
        }
    }
}

/// Valor en SI, positivo, finito y de la dimensión de `unit`, o el problema.
fn positive_value(text: &str, unit: &str) -> Result<f64, String> {
    match (parse_quantity(text), parse_unit(unit)) {
        (Ok(quantity), Ok(expected)) if quantity.dim() != expected.dim => {
            Err(format!("debe ser {unit}: {text}"))
        }
        (Ok(quantity), Ok(_)) if !quantity.si.is_finite() => {
            Err(format!("debe ser finito: {text}"))
        }
        (Ok(quantity), Ok(_)) if quantity.si <= 0.0 => Err(format!("debe ser positivo: {text}")),
        (Ok(quantity), Ok(_)) => Ok(quantity.si),
        (Err(error), _) | (_, Err(error)) => {
            Err(format!("no se puede interpretar ({error}): {text}"))
        }
    }
}

/// Los datos de fuga van todos o ninguno: con la mitad no hay cálculo posible.
/// Devuelve el diámetro de sellado trasero, si se entiende.
fn check_casing_leakage(diagnostics: &mut Diagnostics, casing: &Casing) -> Option<f64> {
    let lengths = [
        ("seal_clearance", casing.seal_clearance.as_str()),
        ("seal_length", casing.seal_length.as_str()),
        ("rear_seal_diameter", casing.rear_seal_diameter.as_str()),
        ("rear_seal_length", casing.rear_seal_length.as_str()),
        (
            "balance_hole_diameter",
            casing.balance_hole_diameter.as_str(),
        ),
        ("balance_hole_circle", casing.balance_hole_circle.as_str()),
    ];
    if !casing.has_leakage() {
        return None;
    }
    if !casing.shaft_seal_diameter.trim().is_empty() {
        casing_value(
            diagnostics,
            "shaft_seal_diameter",
            &casing.shaft_seal_diameter,
            "m",
        );
    }
    let mut rear = None;
    for (name, text) in lengths {
        if text.trim().is_empty() {
            casing_problem(diagnostics, name, "hace falta con los demás datos de fuga");
        } else {
            let value = casing_value(diagnostics, name, text, "m");
            if name == "rear_seal_diameter" {
                rear = value;
            }
        }
    }
    if casing.balance_hole_count == 0 {
        casing_problem(
            diagnostics,
            "balance_hole_count",
            "hace falta con los demás datos de fuga",
        );
    }
    rear
}

/// Cada cota por separado y, si todas se entienden, que encajen entre sí.
fn check_casing(diagnostics: &mut Diagnostics, casing: &Casing) {
    let d2 = casing_value(diagnostics, "d2", &casing.d2, "m");
    let neck = casing_value(diagnostics, "neck_diameter", &casing.neck_diameter, "m");
    let eye = casing_value(
        diagnostics,
        "eye_diameter_max",
        &casing.eye_diameter_max,
        "m",
    );
    let bore = casing_value(diagnostics, "bore_diameter", &casing.bore_diameter, "m");
    let hub = casing_value(diagnostics, "hub_diameter", &casing.hub_diameter, "m");
    casing_value(diagnostics, "b3", &casing.b3, "m");
    let cutwater = casing_value(diagnostics, "cutwater_radius", &casing.cutwater_radius, "m");

    if !matches!(casing.channels, 1 | 2) {
        casing_problem(
            diagnostics,
            "channels",
            &format!("debe ser 1 o 2: {}", casing.channels),
        );
    }
    match (casing.channels, casing.wrap.trim().is_empty()) {
        (1, false) => casing_problem(diagnostics, "wrap", "solo va con 2 canales"),
        (2, true) => casing_problem(diagnostics, "wrap", "hace falta con 2 canales"),
        (2, false) => {
            if let Some(wrap) = casing_value(diagnostics, "wrap", &casing.wrap, "rad") {
                let (low, high) = CASING_WRAP_RANGE_DEG;
                if !(low..=high).contains(&wrap.to_degrees()) {
                    casing_problem(
                        diagnostics,
                        "wrap",
                        &format!("debe estar entre {low} y {high} grados: {}", casing.wrap),
                    );
                }
            }
        }
        _ => {}
    }
    if !casing.throat_area.trim().is_empty() {
        casing_value(diagnostics, "throat_area", &casing.throat_area, "m2");
    }
    let rear_seal = check_casing_leakage(diagnostics, casing);
    if let (Some(rear), Some(hub), Some(d2)) = (rear_seal, hub, d2)
        && !(hub < rear && rear < d2)
    {
        casing_problem(
            diagnostics,
            "rear_seal_diameter",
            "debe quedar entre `hub_diameter` y `d2`",
        );
    }

    if let (Some(d2), Some(neck), Some(eye), Some(bore), Some(hub), Some(cutwater)) =
        (d2, neck, eye, bore, hub, cutwater)
    {
        let nested = [
            ("bore_diameter", "hub_diameter", bore < hub),
            ("hub_diameter", "eye_diameter_max", hub < eye),
            ("eye_diameter_max", "neck_diameter", eye < neck),
            ("neck_diameter", "d2", neck < d2),
        ];
        for (small, large, fits) in nested {
            if !fits {
                casing_problem(diagnostics, small, &format!("debe ser menor que `{large}`"));
            }
        }
        if 2.0 * cutwater <= d2 {
            casing_problem(
                diagnostics,
                "cutwater_radius",
                "deja la lengua dentro del rodete: debe ser mayor que d2/2",
            );
        }
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
