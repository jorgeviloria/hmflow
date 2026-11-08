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
    /// Diámetro del cubo en el plano de entrada, dH, con unidad: de 0 (el
    /// cubo arranca en el eje) a menos que d1. Vacío: el del 1D.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hub_diameter: String,
    /// Posición del borde de ataque sobre la tapa delantera y sobre el cubo:
    /// fracción de la longitud de cada curva desde el plano de entrada, sin
    /// unidad, de 0 a 0,9. Vacío: 0 (el borde en el plano de entrada).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub edge_shroud: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub edge_hub: String,
    /// El borde de ataque como Bézier de 5 puntos, de la tapa al cubo.
    #[serde(default, skip_serializing_if = "MeridianEdge::is_empty")]
    pub edge: MeridianEdge,
    /// Tapa delantera.
    #[serde(default, skip_serializing_if = "MeridianWall::is_empty")]
    pub shroud: MeridianWall,
    /// Tapa trasera (cubo).
    #[serde(default, skip_serializing_if = "MeridianWall::is_empty")]
    pub hub: MeridianWall,
    /// Recorte mecánico del cubo (geometría de fabricación, no hidráulica).
    #[serde(default, skip_serializing_if = "MeridianHubCut::is_empty")]
    pub hub_cut: MeridianHubCut,
    /// Cortes del contorno mecánico de cada tapa sobre su Bézier (geometría
    /// de fabricación, no hidráulica).
    #[serde(default, skip_serializing_if = "MeridianSegments::is_empty")]
    pub segments: MeridianSegments,
}

/// Los cortes de las dos tapas. Vacío: cada tapa es su Bézier entera.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeridianSegments {
    #[serde(default, skip_serializing_if = "MeridianWallSegments::is_empty")]
    pub shroud: MeridianWallSegments,
    #[serde(default, skip_serializing_if = "MeridianWallSegments::is_empty")]
    pub hub: MeridianWallSegments,
    /// Cotas que mandan, como «dz shroud:4 hub:2 = 40 mm»: el hijo (el segundo
    /// nodo) sigue al padre. Van aquí y no en una tapa porque pueden unir las dos.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
}

impl MeridianSegments {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shroud.is_empty() && self.hub.is_empty() && self.constraints.is_empty()
    }
}

/// Qué fija una cota que manda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    /// `z_hijo = z_padre + valor`.
    Dz,
    /// `Ø_hijo = Ø_padre + valor`.
    Dd,
    /// El hijo, sobre la recta que sale del padre con esa dirección.
    Ang,
}

/// Una de las dos tapas del corte meridiano.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WallId {
    Shroud,
    Hub,
}

impl WallId {
    fn name(self) -> &'static str {
        match self {
            Self::Shroud => "shroud",
            Self::Hub => "hub",
        }
    }
}

/// Un nodo del contorno: su tapa y su índice en `nodes`, desde 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeRef {
    pub wall: WallId,
    pub index: usize,
}

impl NodeRef {
    fn parse(text: &str) -> Result<Self, String> {
        let (wall, index) = text
            .split_once(':')
            .ok_or_else(|| format!("cada nodo es «hub:i» o «shroud:i»: {text}"))?;
        let wall = match wall {
            "hub" => WallId::Hub,
            "shroud" => WallId::Shroud,
            _ => return Err(format!("la tapa es hub o shroud: {text}")),
        };
        let index = index
            .parse()
            .map_err(|_| format!("el índice del nodo es un entero desde 0: {text}"))?;
        Ok(Self { wall, index })
    }

    fn to_text(self) -> String {
        format!("{}:{}", self.wall.name(), self.index)
    }
}

/// Una cota que manda, leída de su texto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constraint {
    pub kind: ConstraintKind,
    pub parent: NodeRef,
    pub child: NodeRef,
    /// En metros para `Dz` y `Dd`; en grados, en (−180, 180], para `Ang`.
    pub value: f64,
}

impl Constraint {
    /// Lee «<dz|dd|ang> <padre> <hijo> = <valor> <unidad>».
    ///
    /// # Errors
    ///
    /// El problema, en español, si el texto no tiene esa forma.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (left, right) = text
            .split_once('=')
            .ok_or_else(|| format!("falta «=»: {text}"))?;
        // Se edita a mano: sin distinguir mayúsculas y con espacios junto a «:».
        let left = left
            .to_lowercase()
            .split(':')
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(":");
        let words: Vec<&str> = left.split_whitespace().collect();
        let [kind, parent, child] = words[..] else {
            return Err(format!(
                "antes de «=» van el tipo, el padre y el hijo: {text}"
            ));
        };
        let kind = match kind {
            "dz" => ConstraintKind::Dz,
            "dd" => ConstraintKind::Dd,
            "ang" => ConstraintKind::Ang,
            _ => return Err(format!("el tipo es dz, dd o ang: {text}")),
        };
        let parent = NodeRef::parse(parent)?;
        let child = NodeRef::parse(child)?;
        let right = right.trim();
        let value = match kind {
            ConstraintKind::Dz | ConstraintKind::Dd => length_value(right)?,
            ConstraintKind::Ang => {
                let number = right
                    .len()
                    .checked_sub(3)
                    .filter(|&end| right.is_char_boundary(end))
                    .filter(|&end| right[end..].eq_ignore_ascii_case("deg"))
                    .map(|end| &right[..end])
                    .ok_or_else(|| format!("el ángulo va en deg: {text}"))?;
                match number.trim().parse::<f64>() {
                    Ok(value) if value > -180.0 && value <= 180.0 => value,
                    _ => return Err(format!("el ángulo va de -180 (sin incluir) a 180: {text}")),
                }
            }
        };
        Ok(Self {
            kind,
            parent,
            child,
            value,
        })
    }

    /// El texto de la cota, con las longitudes en mm y los ángulos en deg.
    #[must_use]
    pub fn to_text(self) -> String {
        let round = |value: f64| (value * 1e6).round() / 1e6;
        let (kind, value) = match self.kind {
            ConstraintKind::Dz => ("dz", format!("{} mm", round(self.value * 1000.0))),
            ConstraintKind::Dd => ("dd", format!("{} mm", round(self.value * 1000.0))),
            ConstraintKind::Ang => ("ang", format!("{} deg", round(self.value))),
        };
        format!(
            "{kind} {} {} = {value}",
            self.parent.to_text(),
            self.child.to_text()
        )
    }
}

/// Los cortes de una tapa, como texto: cada uno es el parámetro `t` de la
/// Bézier hidráulica, de 0 a 1 sin incluirlos y en orden creciente. `pieces`
/// dice qué es cada tramo, del arranque al final: `curve` (la Bézier entre
/// sus cortes), `line` (la recta entre ellos) o `arc` (el arco menor de
/// radio dado entre sus dos nodos). Tiene un elemento más que
/// `cuts`; vacío, todos son `curve`. `nodes` da la posición de cada nodo
/// (el arranque, los cortes y el final, uno más que `pieces`) como
/// «z, r» con unidad, por ejemplo `"60 mm, 0 mm"`; vacío, el nodo está
/// sobre la Bézier. Solo un nodo entre rectas o arcos puede ir fuera de ella.
/// `radii` da el radio de redondeo de cada nodo, uno por nodo como `nodes`,
/// con unidad, por ejemplo `"5 mm"`; vacío, la esquina es viva.
/// `chamfers` da el chaflán de cada nodo, uno por nodo como `radii`, con
/// unidad, por ejemplo `"1 mm"`: corta la esquina entre dos rectas a esa
/// distancia de su vértice por cada lado (1x45° con ángulo recto). Un nodo
/// lleva radio o chaflán, no los dos.
/// `arcs` da el radio de cada tramo `arc`, uno por tramo como `pieces`, con
/// unidad y vacío en los demás. El signo dice hacia dónde se curva: positivo
/// a la izquierda de la cuerda recorrida del arranque al final (en el plano
/// z, r), negativo a la derecha.
/// `tangents` dice, uno por tramo como `pieces`, si un arco sale tangente a
/// su vecino: `start` (al tramo anterior, en su nodo de arranque), `end` (al
/// siguiente, en su nodo final) o vacío. Su R se mantiene y el nodo del otro
/// extremo queda sobre su círculo.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeridianWallSegments {
    /// Nodos de la prolongación antes de P0: un entero; vacío, ninguno.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head: String,
    /// Nodos de la prolongación después de P4: un entero; vacío, ninguno.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tail: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub radii: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chamfers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arcs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tangents: Vec<String>,
}

impl MeridianWallSegments {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.head.is_empty()
            && self.tail.is_empty()
            && self.cuts.is_empty()
            && self.pieces.is_empty()
            && self.nodes.is_empty()
            && self.radii.is_empty()
            && self.chamfers.is_empty()
            && self.arcs.is_empty()
            && self.tangents.is_empty()
    }

    /// Los nodos de la prolongación antes de P0; un texto roto cuenta 0 (la
    /// validación lo señala).
    #[must_use]
    pub fn head_count(&self) -> usize {
        self.head.trim().parse().unwrap_or(0)
    }

    /// Los nodos de la prolongación después de P4.
    #[must_use]
    pub fn tail_count(&self) -> usize {
        self.tail.trim().parse().unwrap_or(0)
    }

    /// Cuántos nodos tiene la cadena: la prolongación, P0, los cortes y P4.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.head_count() + self.cuts.len() + 2 + self.tail_count()
    }
}

/// Recorte del cubo, como texto. Con `z` vacío no hay recorte.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeridianHubCut {
    /// z de la cara del recorte, con unidad, desde el plano de entrada.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub z: String,
    /// Vacío o `closed`: cubo cerrado. `through`: agujero pasante.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    /// Diámetro del agujero del eje, con unidad, mayor que 0 (pasante).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub shaft_diameter: String,
    /// Diámetro del avellanado, con unidad, mayor que 0 (pasante).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub counterbore_diameter: String,
    /// Profundidad del avellanado, con unidad, mayor que 0 (pasante).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub counterbore_depth: String,
    /// Chaflán a 45° del agujero, con unidad, mayor que 0 (pasante).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chamfer: String,
}

impl MeridianHubCut {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [
            &self.z,
            &self.mode,
            &self.shaft_diameter,
            &self.counterbore_diameter,
            &self.counterbore_depth,
            &self.chamfer,
        ]
        .iter()
        .all(|text| text.trim().is_empty())
    }
}

/// Lo editado del borde de ataque, como texto: una Bézier de 5 puntos de la
/// tapa (P0) al cubo (P4), medida en el marco de la cuerda. Vacío es la recta.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeridianEdge {
    /// P2 a lo largo de la cuerda: fracción sin unidad de 0 (en P0) a 1 (en
    /// P4). Vacío: 0,5.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub p2_along: String,
    /// P2 separado de la cuerda, con unidad y signo; positivo hacia aguas
    /// arriba. Vacío: 0.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub p2_offset: String,
    /// Ángulo del brazo de inicio (P0→P1) con la cuerda, en grados, sin
    /// unidad, de −90 a 90; positivo hacia aguas arriba. Vacío: 0.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start_angle_deg: String,
    /// Longitud del brazo de inicio, con unidad, mayor que 0. Vacío: la cuarta
    /// parte de la cuerda.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start_length: String,
    /// Ángulo del brazo final (P4→P3) con la cuerda, visto desde P4, igual.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub end_angle_deg: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub end_length: String,
}

impl MeridianEdge {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [
            &self.p2_along,
            &self.p2_offset,
            &self.start_angle_deg,
            &self.start_length,
            &self.end_angle_deg,
            &self.end_length,
        ]
        .iter()
        .all(|text| text.trim().is_empty())
    }
}

/// Lo editado de una tapa, como texto.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeridianWall {
    /// P2: z (con el flujo, 0 en el borde de ataque) y r, con unidad, dentro
    /// del rectángulo de los extremos de la tapa. Van los dos o ninguno.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub p2_z: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub p2_r: String,
    /// Ángulo del brazo de inicio (P0→P1) en grados, sin unidad, de −90 a
    /// 90, medido desde +z hacia +r: 0 axial, 90 radial.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start_angle_deg: String,
    /// Longitud del brazo de inicio, con unidad, mayor que 0.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub start_length: String,
    /// Ángulo del brazo final (P3→P4): el sentido de avance en P4, en grados
    /// de −90 a 90 desde +z hacia +r.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub end_angle_deg: String,
    /// Longitud del brazo final, con unidad, mayor que 0.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub end_length: String,
}

impl MeridianWall {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [
            &self.p2_z,
            &self.p2_r,
            &self.start_angle_deg,
            &self.start_length,
            &self.end_angle_deg,
            &self.end_length,
        ]
        .iter()
        .all(|text| text.trim().is_empty())
    }
}

impl Meridian {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        [&self.hub_diameter, &self.edge_shroud, &self.edge_hub]
            .iter()
            .all(|text| text.trim().is_empty())
            && self.edge.is_empty()
            && self.shroud.is_empty()
            && self.hub.is_empty()
            && self.hub_cut.is_empty()
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
/// positiva), los dos o ninguno; los ángulos de los brazos entre −90 y 90
/// grados y sus longitudes positivas. Que P2 esté dentro del rectángulo de los
/// extremos lo comprueba el diseño, que es quien conoce los extremos.
fn check_meridian(diagnostics: &mut Diagnostics, meridian: &Meridian) {
    let mut problem = |name: String, text: String| {
        diagnostics.push(
            Diagnostic::error("HM-PROJ-031", format!("[meridian] `{name}` {text}"))
                .with_hint("P2 lleva z y r con unidad, como \"24 mm\"; los ángulos van en grados, de -90 a 90, y las longitudes de los brazos con unidad. Vacío: el motor."),
        );
    };
    let diameter = meridian.hub_diameter.trim();
    if !diameter.is_empty()
        && let Err(text) = non_negative_length(diameter)
    {
        problem("hub_diameter".to_string(), text);
    }
    for (name, text) in [
        ("edge_shroud", meridian.edge_shroud.trim()),
        ("edge_hub", meridian.edge_hub.trim()),
    ] {
        if text.is_empty() {
            continue;
        }
        match text.parse::<f64>() {
            Ok(value) if (0.0..=0.9).contains(&value) => {}
            _ => problem(
                name.to_string(),
                format!("debe estar entre 0 y 0.9: {text}"),
            ),
        }
    }
    let edge = &meridian.edge;
    let along = edge.p2_along.trim();
    if !along.is_empty() {
        match along.parse::<f64>() {
            Ok(value) if (0.0..=1.0).contains(&value) => {}
            _ => problem(
                "edge.p2_along".to_string(),
                format!("debe estar entre 0 y 1: {along}"),
            ),
        }
    }
    let offset = edge.p2_offset.trim();
    if !offset.is_empty()
        && let Err(text) = length_value(offset)
    {
        problem("edge.p2_offset".to_string(), text);
    }
    for (name, text) in [
        ("start_angle_deg", edge.start_angle_deg.trim()),
        ("end_angle_deg", edge.end_angle_deg.trim()),
    ] {
        if text.is_empty() {
            continue;
        }
        match text.parse::<f64>() {
            Ok(value) if (-90.0..=90.0).contains(&value) => {}
            _ => problem(
                format!("edge.{name}"),
                format!("debe estar entre -90 y 90 grados: {text}"),
            ),
        }
    }
    for (name, text) in [
        ("start_length", edge.start_length.trim()),
        ("end_length", edge.end_length.trim()),
    ] {
        if !text.is_empty()
            && let Err(error) = positive_value(text, "m")
        {
            problem(format!("edge.{name}"), error);
        }
    }
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
        for (name, text) in [
            ("start_angle_deg", wall.start_angle_deg.trim()),
            ("end_angle_deg", wall.end_angle_deg.trim()),
        ] {
            if text.is_empty() {
                continue;
            }
            match text.parse::<f64>() {
                Ok(value) if (-90.0..=90.0).contains(&value) => {}
                _ => problem(
                    format!("{table}.{name}"),
                    format!("debe estar entre -90 y 90 grados: {text}"),
                ),
            }
        }
        for (name, text) in [
            ("start_length", wall.start_length.trim()),
            ("end_length", wall.end_length.trim()),
        ] {
            if !text.is_empty()
                && let Err(error) = positive_value(text, "m")
            {
                problem(format!("{table}.{name}"), error);
            }
        }
    }
    check_hub_cut(&mut problem, &meridian.hub_cut);
    for (table, wall) in [
        ("segments.shroud", &meridian.segments.shroud),
        ("segments.hub", &meridian.segments.hub),
    ] {
        check_wall_segments(&mut problem, table, wall);
    }
    check_constraints(diagnostics, &meridian.segments);
}

/// HM-PROJ-032: las cotas que mandan se entienden, sus nodos existen, ninguna
/// coordenada tiene dos dueños y no hay ciclos. Que el hijo sea un nodo libre
/// depende de la geometría y lo comprueba la interfaz.
fn check_constraints(diagnostics: &mut Diagnostics, segments: &MeridianSegments) {
    let mut problem = |text: String| {
        diagnostics.push(
            Diagnostic::error(
                "HM-PROJ-032",
                format!("[meridian] `segments.constraints` {text}"),
            )
            .with_hint("Cada cota es «dz|dd|ang padre hijo = valor unidad», como \"dz shroud:4 hub:2 = 40 mm\" o \"ang hub:3 hub:4 = 135 deg\"."),
        );
    };
    let wall = |id: WallId| match id {
        WallId::Shroud => &segments.shroud,
        WallId::Hub => &segments.hub,
    };
    let exists = |node: NodeRef| node.index < wall(node.wall).node_count();
    let mut valid: Vec<(Constraint, &str)> = Vec::new();
    for text in &segments.constraints {
        let constraint = match Constraint::parse(text) {
            Ok(constraint) => constraint,
            Err(error) => {
                problem(error);
                continue;
            }
        };
        let (parent, child) = (constraint.parent, constraint.child);
        let error = if !exists(parent) || !exists(child) {
            Some("el nodo no existe en esa tapa")
        } else if parent == child {
            Some("el padre y el hijo son el mismo nodo")
        } else if constraint.kind == ConstraintKind::Ang
            && !(parent.wall == child.wall
                && parent.index.abs_diff(child.index) == 1
                && wall(parent.wall)
                    .pieces
                    .get(parent.index.min(child.index))
                    .is_some_and(|piece| piece.trim() == "line"))
        {
            Some("el ángulo va entre los dos extremos de una recta")
        } else {
            None
        };
        match error {
            Some(error) => problem(format!("{error}: {text}")),
            None => valid.push((constraint, text.as_str())),
        }
    }
    // Dueños: una cota de cada tipo por hijo, y como mucho dos tipos.
    let mut owners: BTreeMap<NodeRef, [usize; 3]> = BTreeMap::new();
    for (constraint, text) in &valid {
        let count = owners.entry(constraint.child).or_default();
        let slot = match constraint.kind {
            ConstraintKind::Dz => 0,
            ConstraintKind::Dd => 1,
            ConstraintKind::Ang => 2,
        };
        count[slot] += 1;
        if count[slot] > 1 {
            problem(format!(
                "esa coordenada del hijo ya tiene otra cota: {text}"
            ));
        } else if count.iter().all(|&n| n > 0) {
            problem(format!(
                "el hijo tendría dz, dd y ang a la vez; sobra uno: {text}"
            ));
        }
    }
    // Ciclos: recorrido en profundidad del grafo padre → hijo, las dos tapas juntas.
    let mut children: BTreeMap<NodeRef, Vec<(NodeRef, &str)>> = BTreeMap::new();
    for (constraint, text) in &valid {
        children
            .entry(constraint.parent)
            .or_default()
            .push((constraint.child, text));
    }
    let mut state: BTreeMap<NodeRef, bool> = BTreeMap::new(); // false: en curso; true: hecho
    let mut cycles = Vec::new();
    for &start in children.keys() {
        visit(start, &children, &mut state, &mut cycles);
    }
    for text in cycles {
        problem(format!("las cotas forman un ciclo: {text}"));
    }
}

fn visit<'a>(
    node: NodeRef,
    children: &BTreeMap<NodeRef, Vec<(NodeRef, &'a str)>>,
    state: &mut BTreeMap<NodeRef, bool>,
    cycles: &mut Vec<&'a str>,
) {
    if state.contains_key(&node) {
        return;
    }
    state.insert(node, false);
    for &(child, text) in children.get(&node).map_or(&[][..], Vec::as_slice) {
        match state.get(&child) {
            Some(false) => cycles.push(text),
            Some(true) => {}
            None => visit(child, children, state, cycles),
        }
    }
    state.insert(node, true);
}

fn check_wall_segments(
    problem: &mut impl FnMut(String, String),
    table: &str,
    wall: &MeridianWallSegments,
) {
    let mut previous = 0.0;
    for text in &wall.cuts {
        match text.trim().parse::<f64>() {
            Ok(t) if t > previous && t < 1.0 => previous = t,
            _ => {
                problem(
                    format!("{table}.cuts"),
                    format!("deben ir entre 0 y 1, sin incluirlos, en orden creciente: {text}"),
                );
                return;
            }
        }
    }
    for (name, text) in [("head", &wall.head), ("tail", &wall.tail)] {
        if !text.trim().is_empty() && text.trim().parse::<usize>().is_err() {
            problem(
                format!("{table}.{name}"),
                format!("debe ser un entero de cero en adelante: {text}"),
            );
            return;
        }
    }
    let (head, nodes) = (wall.head_count(), wall.node_count());
    if head + wall.tail_count() > 0 && wall.pieces.is_empty() {
        problem(
            format!("{table}.pieces"),
            "con prolongación hay que dar los tramos".to_string(),
        );
    }
    if !wall.pieces.is_empty() && wall.pieces.len() != nodes - 1 {
        problem(
            format!("{table}.pieces"),
            format!(
                "debe tener un tramo más que cortes: {} cortes y {} tramos",
                wall.cuts.len(),
                wall.pieces.len()
            ),
        );
    }
    let first_tail = wall.pieces.len().saturating_sub(wall.tail_count());
    for (i, piece) in wall.pieces.iter().enumerate() {
        let in_extension = i < head || i >= first_tail;
        if in_extension && piece.trim() == "curve" {
            problem(
                format!("{table}.pieces"),
                format!(
                    "un tramo de la prolongación no puede ser curve (tramo {})",
                    i + 1
                ),
            );
        }
    }
    for piece in &wall.pieces {
        if !matches!(piece.trim(), "curve" | "line" | "arc") {
            problem(
                format!("{table}.pieces"),
                format!("cada tramo es curve, line o arc: {piece}"),
            );
        }
    }
    if !wall.nodes.is_empty() && wall.nodes.len() != nodes {
        problem(
            format!("{table}.nodes"),
            format!(
                "debe tener dos nodos más que cortes: {} cortes y {} nodos",
                wall.cuts.len(),
                wall.nodes.len()
            ),
        );
    }
    let first_tail = wall.nodes.len().saturating_sub(wall.tail_count());
    for (i, node) in wall.nodes.iter().map(|node| node.trim()).enumerate() {
        if node.is_empty() {
            if !wall.nodes.is_empty() && (i < head || i >= first_tail) {
                problem(
                    format!("{table}.nodes"),
                    format!("un nodo de la prolongación necesita posición (nodo {i})"),
                );
            }
            continue;
        }
        let parsed = node
            .split_once(',')
            .ok_or_else(|| format!("cada nodo es «z, r» con unidad: {node}"))
            .and_then(|(z, r)| {
                length_value(z.trim())?;
                non_negative_length(r.trim())?;
                Ok(())
            });
        if let Err(text) = parsed {
            problem(format!("{table}.nodes"), text);
        }
    }
    if !wall.radii.is_empty() && wall.radii.len() != nodes {
        problem(
            format!("{table}.radii"),
            format!(
                "debe tener un radio por nodo: {} nodos y {} radios",
                nodes,
                wall.radii.len()
            ),
        );
    }
    for radius in wall.radii.iter().map(|radius| radius.trim()) {
        if radius.is_empty() {
            continue;
        }
        match length_value(radius) {
            Ok(value) if value > 0.0 => {}
            Ok(_) => problem(
                format!("{table}.radii"),
                format!("debe ser positivo: {radius}"),
            ),
            Err(text) => problem(format!("{table}.radii"), text),
        }
    }
    if !wall.chamfers.is_empty() && wall.chamfers.len() != nodes {
        problem(
            format!("{table}.chamfers"),
            format!(
                "debe tener un chaflán por nodo: {} nodos y {} chaflanes",
                nodes,
                wall.chamfers.len()
            ),
        );
    }
    for chamfer in wall.chamfers.iter().map(|chamfer| chamfer.trim()) {
        if chamfer.is_empty() {
            continue;
        }
        match length_value(chamfer) {
            Ok(value) if value > 0.0 => {}
            Ok(_) => problem(
                format!("{table}.chamfers"),
                format!("debe ser positivo: {chamfer}"),
            ),
            Err(text) => problem(format!("{table}.chamfers"), text),
        }
    }
    check_arcs(problem, table, wall);
    check_tangents(problem, table, wall);
}

fn check_tangents(
    problem: &mut impl FnMut(String, String),
    table: &str,
    wall: &MeridianWallSegments,
) {
    let pieces = wall.node_count() - 1;
    if !wall.tangents.is_empty() && wall.tangents.len() != pieces {
        problem(
            format!("{table}.tangents"),
            format!(
                "debe tener un valor por tramo: {pieces} tramos y {} valores",
                wall.tangents.len()
            ),
        );
        return;
    }
    for (i, tangent) in wall.tangents.iter().map(|text| text.trim()).enumerate() {
        if tangent.is_empty() {
            continue;
        }
        let is_arc = wall
            .pieces
            .get(i)
            .is_some_and(|piece| piece.trim() == "arc");
        let message = match tangent {
            _ if !is_arc => format!("el tramo {} no es arc y lleva tangente: {tangent}", i + 1),
            "start" if i == 0 => "el primer tramo no tiene tramo anterior".to_string(),
            "end" if i + 1 == pieces => "el último tramo no tiene tramo siguiente".to_string(),
            "start" | "end" => continue,
            _ => format!("debe ser start, end o vacío: {tangent}"),
        };
        problem(format!("{table}.tangents"), message);
    }
}

fn check_arcs(problem: &mut impl FnMut(String, String), table: &str, wall: &MeridianWallSegments) {
    let pieces = wall.node_count() - 1;
    if !wall.arcs.is_empty() && wall.arcs.len() != pieces {
        problem(
            format!("{table}.arcs"),
            format!(
                "debe tener un radio por tramo: {pieces} tramos y {} radios",
                wall.arcs.len()
            ),
        );
        return;
    }
    for i in 0..pieces {
        let is_arc = wall
            .pieces
            .get(i)
            .is_some_and(|piece| piece.trim() == "arc");
        let radius = wall.arcs.get(i).map_or("", |radius| radius.trim());
        match (is_arc, radius.is_empty()) {
            (true, true) => problem(
                format!("{table}.arcs"),
                format!("el tramo {} es arc y le falta el radio", i + 1),
            ),
            (false, false) => problem(
                format!("{table}.arcs"),
                format!("el tramo {} no es arc y lleva radio: {radius}", i + 1),
            ),
            (true, false) => match length_value(radius) {
                Ok(value) if value != 0.0 => {}
                Ok(_) => problem(
                    format!("{table}.arcs"),
                    format!("no puede ser cero: {radius}"),
                ),
                Err(text) => problem(format!("{table}.arcs"), text),
            },
            (false, true) => {}
        }
    }
}

fn check_hub_cut(problem: &mut impl FnMut(String, String), cut: &MeridianHubCut) {
    if !cut.z.trim().is_empty()
        && let Err(text) = non_negative_length(cut.z.trim())
    {
        problem("hub_cut.z".to_string(), text);
    }
    let mode = cut.mode.trim();
    if !matches!(mode, "" | "closed" | "through") {
        problem(
            "hub_cut.mode".to_string(),
            format!("debe ser closed o through: {mode}"),
        );
    }
    for (name, text) in [
        ("shaft_diameter", cut.shaft_diameter.trim()),
        ("counterbore_diameter", cut.counterbore_diameter.trim()),
        ("counterbore_depth", cut.counterbore_depth.trim()),
        ("chamfer", cut.chamfer.trim()),
    ] {
        if !text.is_empty()
            && let Err(error) = positive_value(text, "m")
        {
            problem(format!("hub_cut.{name}"), error);
        }
    }
}

/// Una longitud finita, cero o positiva.
fn non_negative_length(text: &str) -> Result<f64, String> {
    let value = length_value(text)?;
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(format!("no puede ser negativa: {text}"))
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
