//! Corte meridional del rodete: tapas delantera y trasera y los dos bordes.
//!
//! Cada tapa es una Bézier de grado 4 con 5 puntos (Gülich 2020, §7.2.2.1,
//! pp. 416-418, fig. 7.4, con el borde de ataque en el ojo). Ejes en mm: `z`
//! axial en el sentido del flujo, 0 en el borde de ataque, como mide Gülich
//! zE; `r` radial. Cada punto es `[z, r]`. Como la planta, es lo único que
//! leen el visor y la exportación.
//!
//! - P0 y P4, los extremos, los fija el 1D.
//! - P2 se mueve libre.
//! - P1 solo se desliza por la recta P0–P2 y P3 por la P4–P2, así que la
//!   curvatura es nula en los dos extremos. Se guarda su fracción sobre la
//!   recta: se mueven con P2.

use hm_geometry::{Bezier, Curve, Point};

use crate::plan::{PlanCurve, csv_of, fingerprint_of};
use crate::{DesignError, DesignResult};

/// RDs = (0,6 a 0,8)·b1: el centro del intervalo. Gülich, ec. (7.2), p. 416.
pub const RDS_RATIO: f64 = 0.7;

/// zE = a·b1·nq^x para rodetes en voladizo (OHH). Gülich, ec. (7.2), p. 416,
/// y T7.8.9, p. 526.
pub const ZE_A: f64 = 0.75;
pub const ZE_X: f64 = -0.05;

/// Límites de la fracción de P1 y P3 sobre su recta.
pub const FRACTION_RANGE: (f64, f64) = (0.02, 0.98);

/// Muestras de cada tapa.
const CURVE_POINTS: usize = 121;
/// Muestras para medir el radio mínimo, y para buscarlo, más rápido.
const RADIUS_SAMPLES: usize = 400;
const SEARCH_SAMPLES: usize = 120;

const MM: f64 = 1000.0;

/// Una de las dos tapas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wall {
    Shroud,
    Hub,
}

impl Wall {
    pub const ALL: [Self; 2] = [Self::Shroud, Self::Hub];

    /// Su nombre en el corte y en el CSV.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Shroud => "Tapa delantera",
            Self::Hub => "Tapa trasera",
        }
    }

    /// Su tabla en `[meridian]`.
    #[must_use]
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Shroud => "shroud",
            Self::Hub => "hub",
        }
    }
}

/// Lo editado de una tapa, en mm; `None` es lo del motor.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WallEdit {
    /// P2, `[z, r]`.
    pub p2: Option<[f64; 2]>,
    /// Fracción de P1 sobre P0–P2, en (0, 1).
    pub p1: Option<f64>,
    /// Fracción de P3 sobre P4–P2, en (0, 1).
    pub p3: Option<f64>,
}

/// Lo editado del corte.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MeridianEdits {
    pub shroud: WallEdit,
    pub hub: WallEdit,
}

impl MeridianEdits {
    #[must_use]
    pub fn wall(&self, wall: Wall) -> &WallEdit {
        match wall {
            Wall::Shroud => &self.shroud,
            Wall::Hub => &self.hub,
        }
    }

    pub fn wall_mut(&mut self, wall: Wall) -> &mut WallEdit {
        match wall {
            Wall::Shroud => &mut self.shroud,
            Wall::Hub => &mut self.hub,
        }
    }
}

/// La forma usada de una tapa.
#[derive(Debug, Clone, PartialEq)]
pub struct WallShape {
    /// Los 5 puntos de la Bézier, de P0 a P4.
    pub points: [[f64; 2]; 5],
    pub p1: f64,
    pub p3: f64,
    /// Las fracciones del diseño del motor: las de la curva más suave.
    pub auto_p1: f64,
    pub auto_p3: f64,
    /// Radio de curvatura mínimo a lo largo de la tapa.
    pub min_radius: f64,
}

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
    /// Radio de la tapa delantera que recomienda Gülich, 0,7·b1 (ec. 7.2), de
    /// referencia: con zE de la misma ecuación puede no caber.
    pub r_ds: f64,
    pub shroud: WallShape,
    pub hub: WallShape,
}

impl MeridianParams {
    #[must_use]
    pub fn wall(&self, wall: Wall) -> &WallShape {
        match wall {
            Wall::Shroud => &self.shroud,
            Wall::Hub => &self.hub,
        }
    }
}

/// El corte meridional completo.
#[derive(Debug, Clone, PartialEq)]
pub struct MeridionalModel {
    pub curves: Vec<PlanCurve>,
    pub params: MeridianParams,
}

/// Un punto de control movible: P1, P2 o P3 de una tapa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlPoint {
    pub wall: Wall,
    /// 1, 2 o 3.
    pub index: usize,
}

/// Lo que se escribe al mover un punto de control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Moved {
    /// P2 en `[z, r]`.
    Point([f64; 2]),
    /// P1 o P3, como fracción de su recta.
    Fraction(f64),
}

/// Un punto de control y, si está acoplado, la recta por la que se desliza.
#[derive(Debug, Clone, PartialEq)]
pub struct Handle {
    pub control: ControlPoint,
    pub at: [f64; 2],
    /// De dónde a dónde va su recta: de P0 a P2 para P1, de P4 a P2 para P3.
    pub line: Option<([f64; 2], [f64; 2])>,
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

    /// Los seis puntos de control movibles: P1, P2 y P3 de cada tapa.
    #[must_use]
    pub fn handles(&self) -> Vec<Handle> {
        let mut handles = Vec::with_capacity(6);
        for wall in Wall::ALL {
            let points = self.params.wall(wall).points;
            for index in 1..=3 {
                let line = match index {
                    1 => Some((points[0], points[2])),
                    3 => Some((points[4], points[2])),
                    _ => None,
                };
                handles.push(Handle {
                    control: ControlPoint { wall, index },
                    at: points[index],
                    line,
                });
            }
        }
        handles
    }

    /// Lo que se escribe si el punto de control se lleva a `pointer`: P2 va
    /// donde esté el puntero; P1 y P3, a la proyección sobre su recta, como
    /// fracción dentro de [`FRACTION_RANGE`].
    #[must_use]
    pub fn moved(&self, control: ControlPoint, pointer: [f64; 2]) -> Moved {
        let points = self.params.wall(control.wall).points;
        let (from, to) = match control.index {
            1 => (points[0], points[2]),
            3 => (points[4], points[2]),
            _ => return Moved::Point(pointer),
        };
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let length = dx * dx + dy * dy;
        let fraction = if length > 0.0 {
            ((pointer[0] - from[0]) * dx + (pointer[1] - from[1]) * dy) / length
        } else {
            0.5
        };
        Moved::Fraction(fraction.clamp(FRACTION_RANGE.0, FRACTION_RANGE.1))
    }

    /// CSV para exportar, con columnas `z_mm,r_mm`.
    #[must_use]
    pub fn to_csv(&self) -> String {
        csv_of(&self.curves, "meridiano", ["z_mm", "r_mm"])
    }
}

/// Corte meridional del diseño del motor.
pub fn meridional_model(result: &DesignResult) -> Result<MeridionalModel, DesignError> {
    meridional_model_with(result, &MeridianEdits::default())
}

/// Corte meridional con lo editado.
///
/// Diseño del motor: P2 en la esquina de las tangentes extremas, axial a la
/// entrada y radial a la salida (εDs = εTs = 0, p. 417), y las fracciones de
/// P1 y P3 que hacen cada tapa lo más suave posible (mayor radio mínimo).
/// Gülich pide RDs = 0,6 a 0,8·b1 (p. 416), pero con zE de la ec. (7.2) un
/// giro de 90° de ese radio a menudo no cabe; la p. 417 admite radios
/// menores en bombas pequeñas. Es elección de hmflow.
///
/// Errores: el área de entrada no cabe en el ojo, una fracción fuera de
/// (0, 1), un P2 que no es finito, tapas que se cruzan o un cubo que toca el
/// eje.
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

    let shroud_ends = ([0.0, r1], [z_e, r2]);
    let hub_ends = ([0.0, d1i / 2.0], [z_e + b2, r2]);
    let shroud_corner = [z_e, r1];
    let hub_corner = [z_e + b2, d1i / 2.0];

    let shroud = wall_shape(Wall::Shroud, shroud_ends, shroud_corner, &edits.shroud)?;
    let hub = wall_shape(Wall::Hub, hub_ends, hub_corner, &edits.hub)?;

    let front = sample(&shroud.points)?;
    let rear = sample(&hub.points)?;
    if rear.iter().any(|point| point[1] <= 0.0) {
        return Err(DesignError::Input(
            "meridiano: la tapa trasera toca o cruza el eje".to_string(),
        ));
    }
    if crosses(&front, &rear) {
        return Err(DesignError::Input(
            "meridiano: las tapas delantera y trasera se cruzan".to_string(),
        ));
    }

    let curves = vec![
        PlanCurve {
            name: Wall::Shroud.name().to_string(),
            points: front,
        },
        PlanCurve {
            name: Wall::Hub.name().to_string(),
            points: rear,
        },
        PlanCurve {
            name: "Borde de ataque".to_string(),
            points: vec![[0.0, r1], [0.0, d1i / 2.0]],
        },
        PlanCurve {
            name: "Borde de salida".to_string(),
            points: vec![[z_e + b2, r2], [z_e, r2]],
        },
    ];
    Ok(MeridionalModel {
        curves,
        params: MeridianParams {
            d1i,
            inlet_width,
            z_e,
            r_ds,
            shroud,
            hub,
        },
    })
}

/// Los 5 puntos: P1 a `p1` de P0 hacia P2 y P3 a `p3` de P4 hacia P2.
fn five_points(ends: ([f64; 2], [f64; 2]), p2: [f64; 2], p1: f64, p3: f64) -> [[f64; 2]; 5] {
    let (p0, p4) = ends;
    let toward = |from: [f64; 2], fraction: f64| {
        [
            from[0] + (p2[0] - from[0]) * fraction,
            from[1] + (p2[1] - from[1]) * fraction,
        ]
    };
    [p0, toward(p0, p1), p2, toward(p4, p3), p4]
}

fn bezier(points: &[[f64; 2]; 5]) -> Result<Bezier, DesignError> {
    Bezier::new(points.iter().map(|p| Point::new(p[0], p[1])).collect()).map_err(geometry_failed)
}

fn sample(points: &[[f64; 2]; 5]) -> Result<Vec<[f64; 2]>, DesignError> {
    let curve = bezier(points)?;
    let mut sampled: Vec<[f64; 2]> = curve
        .sample(CURVE_POINTS)
        .map_err(geometry_failed)?
        .into_iter()
        .map(|point| [point.x, point.y])
        .collect();
    // Los extremos, los del 1D sin redondeo.
    sampled[0] = points[0];
    sampled[CURVE_POINTS - 1] = points[4];
    Ok(sampled)
}

/// Radio de curvatura mínimo, sin los extremos, donde la curvatura es nula.
fn min_radius(points: &[[f64; 2]; 5]) -> Result<f64, DesignError> {
    min_radius_sampled(points, RADIUS_SAMPLES)
}

fn min_radius_sampled(points: &[[f64; 2]; 5], samples: usize) -> Result<f64, DesignError> {
    let curve = bezier(points)?;
    let mut radius = f64::INFINITY;
    for index in 1..samples {
        let t = index as f64 / samples as f64;
        let curvature = curve.curvature_at(t).map_err(geometry_failed)?.abs();
        if curvature > 0.0 {
            radius = radius.min(1.0 / curvature);
        }
    }
    Ok(radius)
}

/// Las fracciones de P1 y P3 que hacen la curva más suave: las que dan el
/// mayor radio de curvatura mínimo. Con P1 y P3 pegados a la esquina la curva
/// se dobla en punta; pegados a los extremos, la velocidad se anula allí y la
/// curvatura se dispara. Búsqueda por coordenadas con pasos que se achican.
fn smoothest_fractions(
    ends: ([f64; 2], [f64; 2]),
    corner: [f64; 2],
) -> Result<(f64, f64), DesignError> {
    let radius =
        |p1: f64, p3: f64| min_radius_sampled(&five_points(ends, corner, p1, p3), SEARCH_SAMPLES);
    let (low, high) = FRACTION_RANGE;
    let mut best = (0.5, 0.5);
    let mut best_radius = radius(best.0, best.1)?;
    for step in [0.2, 0.1, 0.05, 0.02, 0.01, 0.005] {
        loop {
            let mut improved = false;
            for (d1, d3) in [(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step)] {
                let candidate = (
                    (best.0 + d1).clamp(low, high),
                    (best.1 + d3).clamp(low, high),
                );
                let value = radius(candidate.0, candidate.1)?;
                if value > best_radius + 1e-9 {
                    best = candidate;
                    best_radius = value;
                    improved = true;
                }
            }
            if !improved {
                break;
            }
        }
    }
    Ok(best)
}

fn wall_shape(
    wall: Wall,
    ends: ([f64; 2], [f64; 2]),
    corner: [f64; 2],
    edit: &WallEdit,
) -> Result<WallShape, DesignError> {
    let (auto_p1, auto_p3) = smoothest_fractions(ends, corner)?;
    let p2 = edit.p2.unwrap_or(corner);
    if !(p2[0].is_finite() && p2[1].is_finite()) {
        return Err(DesignError::Input(format!(
            "meridiano: P2 de la {} no es un punto",
            wall.name().to_lowercase()
        )));
    }
    let fraction = |value: Option<f64>, auto: f64, name: &str| {
        let value = value.unwrap_or(auto);
        if value.is_finite() && value > 0.0 && value < 1.0 {
            Ok(value)
        } else {
            Err(DesignError::Input(format!(
                "meridiano: {name} de la {} vale {value} y tiene que estar entre 0 y 1",
                wall.name().to_lowercase()
            )))
        }
    };
    let p1 = fraction(edit.p1, auto_p1, "P1")?;
    let p3 = fraction(edit.p3, auto_p3, "P3")?;
    let points = five_points(ends, p2, p1, p3);
    Ok(WallShape {
        points,
        p1,
        p3,
        auto_p1,
        auto_p3,
        min_radius: min_radius(&points)?,
    })
}

/// Dos polilíneas se cortan en algún par de segmentos.
fn crosses(first: &[[f64; 2]], second: &[[f64; 2]]) -> bool {
    let side = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    first.windows(2).any(|p| {
        second.windows(2).any(|q| {
            let (d1, d2) = (side(p[0], p[1], q[0]), side(p[0], p[1], q[1]));
            let (d3, d4) = (side(q[0], q[1], p[0]), side(q[0], q[1], p[1]));
            d1 * d2 < 0.0 && d3 * d4 < 0.0
        })
    })
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

fn geometry_failed(error: hm_geometry::GeometryError) -> DesignError {
    DesignError::Input(format!("meridiano: {error}"))
}
