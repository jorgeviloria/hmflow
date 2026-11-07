//! Corte meridional del rodete: tapas delantera y trasera y los dos bordes.
//!
//! Construcción de Gülich 2020, §7.2.2.1, pp. 416-418, fig. 7.4, con el borde
//! de ataque en el ojo. Ejes en mm: `z` axial, 0 en el plano de salida de la
//! tapa trasera y positivo hacia la aspiración; `r` radial. Cada punto es
//! `[z, r]`. Como la planta, es lo único que leen el visor y la exportación.

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
    /// Radio de curvatura de la tapa delantera al salir de g1.
    pub r_ds: f64,
    /// Tramo axial de la tapa delantera tras el borde de ataque.
    pub g1: f64,
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
    let axial_arm = z_e - g1;
    let radial_arm = radial_arm(r2 - r1, axial_arm, r_ds);
    if !(axial_arm > 0.0 && radial_arm > 0.0 && radial_arm < r2 - r1) {
        return Err(DesignError::Input(format!(
            "la tapa delantera no se puede trazar con nq = {:.1}: entre d1 y d2 no cabe el \
             radio RDs (brazo radial {radial_arm:.2} mm, entre 0 y (d2 − d1)/2 = {:.2} mm). \
             Con nq alto el rodete es semiaxial, y esta construcción es la radial de la fig. 7.4",
            result.nq,
            r2 - r1
        )));
    }

    let leading = Point::new(b2 + z_e, r1);
    let end_of_g1 = Point::new(b2 + z_e - g1, r1);
    let outlet = Point::new(b2, r2);
    let straight = Line::new(leading, end_of_g1).map_err(geometry_failed)?;
    let bend = Bezier::cubic(
        end_of_g1,
        Point::new(b2, r1),
        Point::new(b2, r2 - radial_arm),
        outlet,
    )
    .map_err(geometry_failed)?;

    // Tapa trasera: del cubo en el borde de ataque, axial, a (0, d1/2), radial
    // y sin curvatura; luego el disco plano hasta d2 (εTs = 0).
    let hub = Point::new(leading.x, d1i / 2.0);
    let hub_bend = Bezier::cubic(
        hub,
        Point::new(0.0, d1i / 2.0),
        Point::new(0.0, (d1i / 2.0 + r1) / 2.0),
        Point::new(0.0, r1),
    )
    .map_err(geometry_failed)?;
    let disc = Line::new(Point::new(0.0, r1), Point::new(0.0, r2)).map_err(geometry_failed)?;

    let mut front = straight.sample(G1_POINTS).map_err(geometry_failed)?;
    front.extend(&bend.sample(BEZIER_POINTS).map_err(geometry_failed)?[1..]);
    let mut rear = hub_bend.sample(BEZIER_POINTS).map_err(geometry_failed)?;
    rear.extend(&disc.sample(DISC_POINTS).map_err(geometry_failed)?[1..]);
    let points = |curve: &[Point]| curve.iter().map(|point| [point.x, point.y]).collect();

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
            points: vec![[leading.x, r1], [leading.x, d1i / 2.0]],
        },
        PlanCurve {
            name: "Borde de salida".to_string(),
            points: vec![[0.0, r2], [b2, r2]],
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
        },
    })
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
