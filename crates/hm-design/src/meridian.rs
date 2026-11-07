//! Corte meridional del rodete: tapas delantera y trasera y los dos bordes.
//!
//! Construcción de Gülich 2020, §7.2.2.1, pp. 416-418, fig. 7.4, con el borde
//! de ataque en el ojo. Ejes en mm: `z` axial en el sentido del flujo, 0 en el
//! borde de ataque, como mide Gülich zE; `r` radial. La entrada queda a la
//! izquierda y la salida a la derecha, como la fig. 7.4. Cada punto es
//! `[z, r]`. Como la planta, es lo único que leen el visor y la exportación.
//!
//! La construcción se hace con z hacia la aspiración y 0 en la tapa trasera;
//! al entregar los puntos se da la vuelta al eje.

use hm_geometry::{Bezier, Curve, Line, Point};

use crate::plan::{PlanCurve, csv_of, fingerprint_of};
use crate::{DesignError, DesignResult};

/// RDs = (0,6 a 0,8)·b1: el centro del intervalo. Gülich, ec. (7.2), p. 416.
pub const RDS_RATIO: f64 = 0.7;

/// g1 = (0,2 a 0,3)·b1: el centro del intervalo. Gülich, p. 416.
pub const G1_RATIO: f64 = 0.25;

/// zE = a·b1·nq^x para rodetes en voladizo (OHH). Gülich, ec. (7.2), p. 416,
/// y T7.8.9, p. 526.
pub const ZE_A: f64 = 0.75;
pub const ZE_X: f64 = -0.05;

/// Puntos del tramo g1, de cada Bézier y del disco plano de la tapa trasera.
const G1_POINTS: usize = 5;
const BEZIER_POINTS: usize = 101;
const DISC_POINTS: usize = 21;

const MM: f64 = 1000.0;

/// Las cotas con que se construyó el corte, en mm.
#[derive(Debug, Clone, PartialEq)]
pub struct MeridianParams {
    /// Diámetro del borde de ataque en la tapa trasera: el que da el área de
    /// entrada del 1D, π·(d1² − d1i²)/4 = π·d1·b1.
    pub d1i: f64,
    /// Anchura del canal en el borde de ataque, (d1 − d1i)/2: el b1 de Gülich.
    pub inlet_width: f64,
    /// Del borde de ataque al plano de salida de la tapa delantera.
    pub z_e: f64,
    /// Radio de curvatura de la tapa delantera al salir de g1: el de la regla
    /// de Gülich sin ediciones, o el que resulta de los brazos editados.
    pub r_ds: f64,
    /// Tramo axial de la tapa delantera tras el borde de ataque.
    pub g1: f64,
    /// Brazos usados en las dos Bézier, editados o automáticos.
    pub front_axial_arm: f64,
    pub front_radial_arm: f64,
    pub rear_axial_arm: f64,
    pub rear_radial_arm: f64,
}

/// Ediciones del corte con enlace tangente, en mm. Cada punto de control
/// interior se desliza por su recta, así que ninguna tangencia se rompe:
///
/// - tapa delantera: el brazo axial va del final de g1 hacia la esquina, sobre
///   r = d1/2; el radial baja desde d2, sobre el plano de salida;
/// - tapa trasera: el brazo axial va del cubo hacia el disco, sobre r = d1i/2;
///   el radial baja desde la unión con el disco en d1/2, sobre el disco.
///
/// `None` es el valor automático. Los extremos los fija el 1D.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeridianEdits {
    pub front_axial_arm: Option<f64>,
    pub front_radial_arm: Option<f64>,
    pub rear_axial_arm: Option<f64>,
    pub rear_radial_arm: Option<f64>,
}

/// El corte meridional completo.
#[derive(Debug, Clone, PartialEq)]
pub struct MeridionalModel {
    pub curves: Vec<PlanCurve>,
    pub params: MeridianParams,
}

impl MeridionalModel {
    /// La curva con ese nombre.
    #[must_use]
    pub fn curve(&self, name: &str) -> Option<&PlanCurve> {
        self.curves.iter().find(|curve| curve.name == name)
    }

    /// La misma huella que la planta, sobre estas curvas.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        fingerprint_of(&self.curves)
    }

    /// La huella en 16 dígitos hexadecimales.
    #[must_use]
    pub fn fingerprint_text(&self) -> String {
        format!("{:016x}", self.fingerprint())
    }

    /// El canal a lo largo de la línea media, una estación por punto de la
    /// tapa delantera. Cada estación une ese punto con el de la trasera que
    /// está a la misma fracción de su longitud: en el borde de ataque une sus
    /// extremos y en la salida, los de d2. El punto medio está en la línea
    /// media, la distancia es la anchura b, y A = 2π·r·b (Gülich, p. 417). Es
    /// geometría del corte, sin el bloqueo de los álabes.
    #[must_use]
    pub fn channel(&self) -> Vec<ChannelStation> {
        let (Some(front), Some(rear)) = (self.curve("Tapa delantera"), self.curve("Tapa trasera"))
        else {
            return Vec::new();
        };
        let front_length = cumulative(&front.points);
        let rear_length = cumulative(&rear.points);
        let (front_total, rear_total) = (
            front_length[front_length.len() - 1],
            rear_length[rear_length.len() - 1],
        );
        let mut stations: Vec<ChannelStation> = Vec::with_capacity(front.points.len());
        let mut previous: Option<[f64; 2]> = None;
        for (point, length) in front.points.iter().zip(&front_length) {
            let partner = at_length(
                &rear.points,
                &rear_length,
                length / front_total * rear_total,
            );
            let middle = [(point[0] + partner[0]) / 2.0, (point[1] + partner[1]) / 2.0];
            let width = (point[0] - partner[0]).hypot(point[1] - partner[1]);
            let m = match (previous, stations.last()) {
                (Some(before), Some(last)) => {
                    last.m + (middle[0] - before[0]).hypot(middle[1] - before[1])
                }
                _ => 0.0,
            };
            previous = Some(middle);
            stations.push(ChannelStation {
                m,
                r: middle[1],
                width,
                area: 2.0 * std::f64::consts::PI * middle[1] * width,
            });
        }
        stations
    }

    /// CSV para exportar, con columnas `z_mm,r_mm`.
    #[must_use]
    pub fn to_csv(&self) -> String {
        csv_of(&self.curves, "meridiano", ["z_mm", "r_mm"])
    }
}

/// Corte meridional de un diseño.
///
/// 1. Tapa delantera: recta axial g1 en r = d1/2 desde el borde de ataque, y
///    una Bézier cúbica hasta (b2, d2/2). El primer punto de control va en la
///    esquina (z = b2, r = d1/2), así que llega a d2 en radial y sin
///    curvatura; el segundo da el radio RDs al salir de g1. Gülich no da regla
///    para esta curva (p. 417): la Bézier es elección de hmflow.
/// 2. Tapa trasera: del cubo en el borde de ataque (d1i), una Bézier con el
///    punto de control en la esquina hasta (0, d1/2), donde llega radial y sin
///    curvatura; luego plana hasta d2. Es la fig. 7.4 con εTs = 0; Gülich no
///    da RTS, y unirla en d1/2 es elección de hmflow. La anchura del canal es
///    un resultado: baja de la de entrada a b2 sin subir.
/// 3. Bordes: el de ataque, radial en z = b2 + zE; el de salida, en d2.
///
/// εDs = εTs = 0: la p. 417 los pone en cero con nq < 20 y < 30, y no da ley
/// para nq mayores.
pub fn meridional_model(result: &DesignResult) -> Result<MeridionalModel, DesignError> {
    meridional_model_with(result, &MeridianEdits::default())
}

/// El corte con los brazos editados. Sin ediciones es [`meridional_model`].
/// Un brazo fuera de su recta es un error. Las tapas no pueden cruzarse: con
/// el enlace tangente la trasera queda en r ≤ d1/2 hasta el disco, y la
/// delantera en r ≥ d1/2 y antes del disco.
pub fn meridional_model_with(
    result: &DesignResult,
    edits: &MeridianEdits,
) -> Result<MeridionalModel, DesignError> {
    let d1 = result.d1.si * MM;
    let b1 = result.b1.si * MM;
    let r1 = d1 / 2.0;
    let r2 = result.d2.si * MM / 2.0;
    let b2 = result.b2.si * MM;

    let hub_squared = d1 * d1 - 4.0 * d1 * b1;
    if !hub_squared.is_finite() || hub_squared <= 0.0 {
        return Err(DesignError::Input(format!(
            "el área de entrada π·d1·b1 no cabe en el ojo: b1 = {b1:.2} mm es al menos d1/4 = {:.2} mm",
            d1 / 4.0
        )));
    }
    let d1i = hub_squared.sqrt();
    let inlet_width = (d1 - d1i) / 2.0;
    let z_e = ZE_A * inlet_width * result.nq.powf(ZE_X);
    let r_ds = RDS_RATIO * inlet_width;
    let g1 = G1_RATIO * inlet_width;

    // Brazos de la Bézier: axial hasta la esquina y radial desde d2.
    let corner = z_e - g1;
    let axial_arm = arm(
        edits.front_axial_arm,
        corner,
        corner,
        "brazo axial de la tapa delantera",
    )?;
    let rule_arm = radial_arm(r2 - r1, axial_arm, r_ds);
    if edits.front_radial_arm.is_none()
        && !(axial_arm > 0.0 && rule_arm > 0.0 && rule_arm < r2 - r1)
    {
        let radial_arm = rule_arm;
        return Err(DesignError::Input(format!(
            "la tapa delantera no se puede trazar con nq = {:.1}: entre d1 y d2 no cabe el \
             radio RDs (brazo radial {radial_arm:.2} mm, entre 0 y (d2 − d1)/2 = {:.2} mm). \
             Con nq alto el rodete es semiaxial, y esta construcción es la radial de la fig. 7.4",
            result.nq,
            r2 - r1
        )));
    }
    let radial_arm = arm(
        edits.front_radial_arm,
        rule_arm,
        r2 - r1,
        "brazo radial de la tapa delantera",
    )?;
    if radial_arm >= r2 - r1 {
        return Err(out_of_line(
            "brazo radial de la tapa delantera",
            radial_arm,
            r2 - r1,
        ));
    }
    // El radio al salir de g1: el de la regla, o el que dan los brazos.
    let r_ds = if edits.front_radial_arm.is_none() && edits.front_axial_arm.is_none() {
        r_ds
    } else {
        1.5 * axial_arm * axial_arm / (r2 - r1 - radial_arm)
    };

    let leading = Point::new(b2 + z_e, r1);
    let end_of_g1 = Point::new(b2 + z_e - g1, r1);
    let outlet = Point::new(b2, r2);
    let straight = Line::new(leading, end_of_g1).map_err(geometry_failed)?;
    let bend = Bezier::cubic(
        end_of_g1,
        // Sin edición, la esquina exacta.
        Point::new(
            edits.front_axial_arm.map_or(b2, |arm| end_of_g1.x - arm),
            r1,
        ),
        Point::new(b2, r2 - radial_arm),
        outlet,
    )
    .map_err(geometry_failed)?;

    // Tapa trasera: del cubo en el borde de ataque, axial, a (0, d1/2), radial
    // y sin curvatura; luego el disco plano hasta d2 (εTs = 0).
    let hub = Point::new(leading.x, d1i / 2.0);
    let rear_axial_arm = arm(
        edits.rear_axial_arm,
        leading.x,
        leading.x,
        "brazo axial de la tapa trasera",
    )?;
    let rear_radial_arm = arm(
        edits.rear_radial_arm,
        (r1 - d1i / 2.0) / 2.0,
        r1 - d1i / 2.0,
        "brazo radial de la tapa trasera",
    )?;
    let hub_bend = Bezier::cubic(
        hub,
        Point::new(hub.x - rear_axial_arm, d1i / 2.0),
        Point::new(
            0.0,
            edits
                .rear_radial_arm
                .map_or((d1i / 2.0 + r1) / 2.0, |arm| r1 - arm),
        ),
        Point::new(0.0, r1),
    )
    .map_err(geometry_failed)?;
    let disc = Line::new(Point::new(0.0, r1), Point::new(0.0, r2)).map_err(geometry_failed)?;

    let mut front = straight.sample(G1_POINTS).map_err(geometry_failed)?;
    front.extend(&bend.sample(BEZIER_POINTS).map_err(geometry_failed)?[1..]);
    let mut rear = hub_bend.sample(BEZIER_POINTS).map_err(geometry_failed)?;
    rear.extend(&disc.sample(DISC_POINTS).map_err(geometry_failed)?[1..]);
    // z en el sentido del flujo, 0 en el borde de ataque.
    let flow = |z: f64| leading.x - z;
    let points = |curve: &[Point]| curve.iter().map(|point| [flow(point.x), point.y]).collect();

    let curves = vec![
        PlanCurve {
            name: "Tapa delantera".to_string(),
            points: points(&front),
        },
        PlanCurve {
            name: "Tapa trasera".to_string(),
            points: points(&rear),
        },
        PlanCurve {
            name: "Borde de ataque".to_string(),
            points: vec![[0.0, r1], [0.0, d1i / 2.0]],
        },
        PlanCurve {
            name: "Borde de salida".to_string(),
            points: vec![[flow(0.0), r2], [flow(b2), r2]],
        },
    ];
    Ok(MeridionalModel {
        curves,
        params: MeridianParams {
            d1i,
            inlet_width,
            z_e,
            r_ds,
            g1,
            front_axial_arm: axial_arm,
            front_radial_arm: radial_arm,
            rear_axial_arm,
            rear_radial_arm,
        },
    })
}

/// El brazo editado o el automático, comprobado: mayor que 0 y no más que
/// `max`, para que el punto de control no salga de su recta.
fn arm(edited: Option<f64>, auto: f64, max: f64, name: &str) -> Result<f64, DesignError> {
    let value = edited.unwrap_or(auto);
    if value.is_finite() && value > 0.0 && value <= max {
        Ok(value)
    } else {
        Err(out_of_line(name, value, max))
    }
}

fn out_of_line(name: &str, value: f64, max: f64) -> DesignError {
    DesignError::Input(format!(
        "meridiano: el {name} vale {value:.2} mm y tiene que estar entre 0 y {max:.2} mm"
    ))
}

/// Una estación del canal sobre la línea media, en mm y mm².
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelStation {
    /// Longitud desarrollada de la línea media desde el borde de ataque.
    pub m: f64,
    /// Radio de la línea media.
    pub r: f64,
    /// Anchura del canal, de tapa a tapa.
    pub width: f64,
    /// Área de paso, 2π·r·b.
    pub area: f64,
}

impl ChannelStation {
    /// Velocidad meridional, m/s, con el caudal que pasa por el rodete en
    /// m³/s (Qth, con las fugas). Sin bloqueo de álabes.
    #[must_use]
    pub fn cm(&self, flow_m3s: f64) -> f64 {
        flow_m3s / (self.area * 1e-6)
    }
}

/// Longitud acumulada de una polilínea en cada punto.
fn cumulative(line: &[[f64; 2]]) -> Vec<f64> {
    let mut lengths = vec![0.0];
    for pair in line.windows(2) {
        let last = lengths[lengths.len() - 1];
        lengths.push(last + (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]));
    }
    lengths
}

/// El punto de la polilínea a esa longitud desde su inicio. En los extremos
/// devuelve el punto exacto.
fn at_length(line: &[[f64; 2]], lengths: &[f64], target: f64) -> [f64; 2] {
    if target <= 0.0 {
        return line[0];
    }
    for index in 1..line.len() {
        if lengths[index] >= target {
            let span = lengths[index] - lengths[index - 1];
            let t = if span > 0.0 {
                (target - lengths[index - 1]) / span
            } else {
                0.0
            };
            let (a, b) = (line[index - 1], line[index]);
            return [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
        }
    }
    line[line.len() - 1]
}

/// Brazo radial de la Bézier para que su radio de curvatura al salir de g1
/// sea `r_ds`. En una cúbica, κ(0) = (2/3)·|P1P0 × P2P1|/|P1P0|³; con P1 en
/// la esquina, |P1P0| es el brazo axial y el producto vale brazo axial ×
/// (subida − brazo radial).
fn radial_arm(rise: f64, axial_arm: f64, r_ds: f64) -> f64 {
    rise - 1.5 * axial_arm * axial_arm / r_ds
}

fn geometry_failed(error: hm_geometry::GeometryError) -> DesignError {
    DesignError::Input(format!("meridiano: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La fórmula del brazo radial da el radio RDs en el arranque de la curva,
    /// medido sobre la Bézier con diferencias finitas.
    #[test]
    fn the_radial_arm_gives_the_requested_radius() {
        let (r1, r2, b2, z_start, r_ds) = (52.0, 100.0, 18.0, 32.0, 26.0);
        let axial_arm = z_start - b2;
        let radial_arm = radial_arm(r2 - r1, axial_arm, r_ds);
        let bend = Bezier::cubic(
            Point::new(z_start, r1),
            Point::new(b2, r1),
            Point::new(b2, r2 - radial_arm),
            Point::new(b2, r2),
        )
        .unwrap();
        let h = 1e-4;
        let p0 = bend.point_at(0.0).unwrap();
        let p1 = bend.point_at(h).unwrap();
        let p2 = bend.point_at(2.0 * h).unwrap();
        let (dx, dy) = (
            (-3.0 * p0.x + 4.0 * p1.x - p2.x) / (2.0 * h),
            (-3.0 * p0.y + 4.0 * p1.y - p2.y) / (2.0 * h),
        );
        let (ddx, ddy) = (
            (p0.x - 2.0 * p1.x + p2.x) / (h * h),
            (p0.y - 2.0 * p1.y + p2.y) / (h * h),
        );
        let radius = (dx * dx + dy * dy).powf(1.5) / (dx * ddy - dy * ddx).abs();
        assert!((radius - r_ds).abs() / r_ds < 1e-3, "{radius}");
    }
}
