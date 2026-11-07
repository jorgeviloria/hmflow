//! Vista en planta del rodete y la voluta del modelo, en milímetros.
//!
//! Es lo único que leen el visor y la exportación: los dos dibujan estos
//! puntos y no calculan ninguno. Así no pueden separarse. La huella identifica
//! la geometría, y sale en el visor y en lo exportado.

use std::f64::consts::TAU;

use hm_geometry::{Arc, Curve, GeometryError, Point};

use crate::DesignResult;
use crate::geometry::Geometry;

/// Segmentos de cada círculo: 120 dan un polígono que no se nota a la vista.
const CIRCLE_SEGMENTS: usize = 120;

/// Paso con que se interpola la voluta entre sus secciones, en grados.
const VOLUTE_STEP_DEG: f64 = 3.0;

const MM: f64 = 1000.0;

/// Una curva con nombre, ya muestreada, en mm.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanCurve {
    pub name: String,
    pub points: Vec<[f64; 2]>,
}

/// La vista en planta completa.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanModel {
    pub curves: Vec<PlanCurve>,
}

impl PlanModel {
    /// La curva con ese nombre.
    #[must_use]
    pub fn curve(&self, name: &str) -> Option<&PlanCurve> {
        self.curves.iter().find(|curve| curve.name == name)
    }

    /// FNV-1a de 64 bits sobre nombres, número de puntos y los bits de cada
    /// coordenada.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        fingerprint_of(&self.curves)
    }

    /// La huella como se muestra y se escribe: 16 dígitos hexadecimales.
    #[must_use]
    pub fn fingerprint_text(&self) -> String {
        format!("{:016x}", self.fingerprint())
    }

    /// CSV para exportar, con columnas `x_mm,y_mm`.
    #[must_use]
    pub fn to_csv(&self) -> String {
        csv_of(&self.curves, "vista en planta", ["x_mm", "y_mm"])
    }
}

/// FNV-1a de 64 bits sobre nombres, número de puntos y los bits de cada
/// coordenada. Cualquier cambio, incluso en la última cifra, la mueve. El
/// número de puntos separa una curva de la siguiente.
pub(crate) fn fingerprint_of(curves: &[PlanCurve]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    for curve in curves {
        feed(curve.name.as_bytes());
        feed(&(curve.points.len() as u64).to_le_bytes());
        for point in &curve.points {
            feed(&point[0].to_bits().to_le_bytes());
            feed(&point[1].to_bits().to_le_bytes());
        }
    }
    hash
}

/// CSV para exportar: UTF-8 con BOM, para que Excel lea las tildes; una
/// cabecera con la vista y la huella; luego `curva,punto` y las dos columnas,
/// una fila por punto. Cada número se escribe con la forma más corta que,
/// releída, da el mismo `f64`, con exponente si es muy pequeño o muy grande:
/// el archivo releído tiene la misma huella.
pub(crate) fn csv_of(curves: &[PlanCurve], view: &str, columns: [&str; 2]) -> String {
    use std::fmt::Write as _;
    let mut out = format!(
        "\u{feff}# hmflow {view}, mm, huella {:016x}\ncurva,punto,{},{}\n",
        fingerprint_of(curves),
        columns[0],
        columns[1]
    );
    for curve in curves {
        let name = csv_field(&curve.name);
        for (index, [x, y]) in curve.points.iter().enumerate() {
            // Escribir en un String no falla.
            let _ = writeln!(out, "{name},{index},{x:?},{y:?}");
        }
    }
    out
}

/// Un campo CSV: entre comillas, con las comillas dobladas, si lleva coma,
/// comillas o salto de línea (RFC 4180).
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// Planta de un diseño: círculos de D1, D2 y lengua, un álabe por canal y la
/// voluta del modelo, que con carcasa no existe.
pub fn plan_model(result: &DesignResult, geometry: &Geometry) -> Result<PlanModel, GeometryError> {
    let mut curves = vec![
        circle("D1", result.d1.si / 2.0)?,
        circle("D2", result.d2.si / 2.0)?,
        circle("Lengua", geometry.outlet.cutwater_radius)?,
    ];

    let blades = result.selection.blade_count;
    for blade in 0..blades {
        let offset = TAU * f64::from(blade) / f64::from(blades);
        let points = geometry
            .impeller
            .blade
            .iter()
            .map(|point| {
                let angle = offset - point.theta;
                [point.r * MM * angle.cos(), point.r * MM * angle.sin()]
            })
            .collect();
        curves.push(PlanCurve {
            name: format!("Álabe {}", blade + 1),
            points,
        });
    }

    if let Some(model) = &result.model_volute {
        let mut stations = vec![(0.0, result.volute.base_radius.si)];
        stations.extend(
            model
                .sections
                .iter()
                .map(|section| (section.angle_deg, section.outer_radius.si)),
        );
        curves.push(PlanCurve {
            name: "Voluta".to_string(),
            points: volute_outline(&stations),
        });
    }
    Ok(PlanModel { curves })
}

fn circle(name: &str, radius_m: f64) -> Result<PlanCurve, GeometryError> {
    let arc = Arc::new(Point::new(0.0, 0.0), radius_m * MM, 0.0, TAU)?;
    let points = arc
        .sample(CIRCLE_SEGMENTS + 1)?
        .into_iter()
        .map(|point| [point.x, point.y])
        .collect();
    Ok(PlanCurve {
        name: name.to_string(),
        points,
    })
}

/// Del radio base en la lengua (0°) a los radios exteriores de las secciones,
/// con el radio interpolado en línea recta cada `VOLUTE_STEP_DEG`.
fn volute_outline(stations: &[(f64, f64)]) -> Vec<[f64; 2]> {
    let at = |angle_deg: f64, radius_m: f64| {
        let angle = angle_deg.to_radians();
        [radius_m * MM * angle.cos(), radius_m * MM * angle.sin()]
    };
    let mut points = Vec::new();
    for pair in stations.windows(2) {
        let ((a0, r0), (a1, r1)) = (pair[0], pair[1]);
        let steps = ((a1 - a0) / VOLUTE_STEP_DEG).ceil().max(1.0) as usize;
        for step in 0..steps {
            let t = step as f64 / steps as f64;
            points.push(at(a0 + (a1 - a0) * t, r0 + (r1 - r0) * t));
        }
    }
    if let Some(&(angle, radius)) = stations.last() {
        points.push(at(angle, radius));
    }
    points
}
