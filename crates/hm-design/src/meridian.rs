//! Corte meridional del rodete: tapas delantera y trasera y los dos bordes.
//!
//! Cada tapa es una Bézier de grado 4 con 5 puntos (Gülich 2020, §7.2.2.1,
//! pp. 416-418, fig. 7.4, con el borde de ataque en el ojo). Ejes en mm: `z`
//! axial en el sentido del flujo, 0 en el borde de ataque, como mide Gülich
//! zE; `r` radial. Cada punto es `[z, r]`. Como la planta, es lo único que
//! leen el visor y la exportación.
//!
//! - P0 y P4, los extremos, los fija el 1D.
//! - P1 sale de P0 y P3 llega a P4 con un ángulo y una longitud propios (los
//!   brazos). El ángulo va desde +z hacia +r, en grados: 0° es axial, 90°
//!   radial; en P4 es el sentido de avance de la curva. No dependen de P2.
//! - P2 se mueve libre dentro del rectángulo que forman P0 y P4, para que la
//!   curva no se salga de él.
//!
//! En el diseño del motor los brazos son axial a la entrada y radial a la
//! salida y P2 está en la esquina: P1 y P3 quedan sobre las rectas a P2 y la
//! curvatura es nula en los extremos. Si se inclina un brazo ya no lo es.

use hm_geometry::{Bezier, Curve, Point};

use crate::plan::{PlanCurve, csv_of, fingerprint_of};
use crate::{DesignError, DesignResult};

/// RDs = (0,6 a 0,8)·b1: el centro del intervalo. Gülich, ec. (7.2), p. 416.
pub const RDS_RATIO: f64 = 0.7;

/// zE = a·b1·nq^x para rodetes en voladizo (OHH). Gülich, ec. (7.2), p. 416,
/// y T7.8.9, p. 526.
pub const ZE_A: f64 = 0.75;
pub const ZE_X: f64 = -0.05;

/// Límites del ángulo de un brazo, en grados: sin volver hacia atrás ni hacia
/// el eje.
pub const ANGLE_RANGE: (f64, f64) = (-90.0, 90.0);

/// Ángulo de inicio y final del diseño del motor, en grados (εDs = εTs = 0,
/// p. 417): axial a la entrada, radial a la salida.
pub const AUTO_START_ANGLE: f64 = 0.0;
pub const AUTO_END_ANGLE: f64 = 90.0;

/// Dónde puede estar un extremo del borde de ataque sobre su curva, como
/// fracción de su longitud desde la entrada: 0 es el plano de entrada.
pub const EDGE_FRACTION_RANGE: (f64, f64) = (0.0, 0.9);

/// Longitud mínima de un brazo al arrastrarlo, en mm.
pub const MIN_ARM: f64 = 0.1;

/// Fracciones de la búsqueda de las longitudes del motor, sobre la distancia de
/// cada extremo a la esquina.
const SEARCH_FRACTIONS: (f64, f64) = (0.02, 0.98);

/// Cota de tolerancia para estar dentro del rectángulo de P2, en mm.
const RANGE_TOLERANCE: f64 = 1e-9;

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
    /// P2, `[z, r]`, dentro del rectángulo de los extremos.
    pub p2: Option<[f64; 2]>,
    /// Ángulo del brazo de inicio (P0→P1), en grados dentro de
    /// [`ANGLE_RANGE`].
    pub start_angle: Option<f64>,
    /// Longitud del brazo de inicio, en mm, mayor que 0.
    pub start_length: Option<f64>,
    /// Ángulo del brazo final (P3→P4), en grados dentro de [`ANGLE_RANGE`].
    pub end_angle: Option<f64>,
    /// Longitud del brazo final, en mm, mayor que 0.
    pub end_length: Option<f64>,
}

/// Lo editado del corte.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MeridianEdits {
    /// Diámetro del cubo en el plano de entrada, dH, en mm: de 0 (el cubo
    /// arranca en el eje) a menos que d1. `None` es el que da el área del 1D.
    pub hub_diameter: Option<f64>,
    /// Posición del borde de ataque sobre la tapa delantera y sobre el cubo,
    /// como fracción de la longitud de cada curva desde el plano de entrada,
    /// dentro de [`EDGE_FRACTION_RANGE`]. `None` es 0.
    pub edge_shroud: Option<f64>,
    pub edge_hub: Option<f64>,
    /// Cuánto se abomba el borde de ataque, en mm: la Bézier cúbica del borde
    /// lleva P1 en el tercio de la cuerda, cerca de la tapa, y P2 en los dos
    /// tercios, cerca del cubo, cada uno desplazado esta distancia en
    /// perpendicular a la cuerda. Positivo abomba hacia aguas arriba. `None`
    /// es 0: el borde recto.
    pub edge_bow_shroud: Option<f64>,
    pub edge_bow_hub: Option<f64>,
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
    /// Brazos usados: ángulos en grados y longitudes en mm.
    pub start_angle: f64,
    pub start_length: f64,
    pub end_angle: f64,
    pub end_length: f64,
    /// Longitudes del diseño del motor: las de la curva más suave. Los
    /// ángulos del motor son [`AUTO_START_ANGLE`] y [`AUTO_END_ANGLE`].
    pub auto_start_length: f64,
    pub auto_end_length: f64,
    /// Rectángulo de P2: esquinas `[z, r]` mínima y máxima, las de P0 y P4.
    pub p2_range: ([f64; 2], [f64; 2]),
    /// Radio de curvatura mínimo a lo largo de la tapa.
    pub min_radius: f64,
    /// Dónde está ese radio mínimo, o sea la curvatura máxima: `[z, r]`.
    pub max_curvature_at: [f64; 2],
}

/// Las cotas con que se construyó el corte, en mm.
#[derive(Debug, Clone, PartialEq)]
pub struct MeridianParams {
    /// Diámetro del cubo en el plano de entrada, dH: el editado o, si no, el que
    /// da el área del 1D con el borde radial, π·(d1² − d1i²)/4 = π·d1·b1.
    pub d1i: f64,
    /// El d1i del 1D, el que usa el motor cuando no se edita dH.
    pub auto_d1i: f64,
    /// Anchura del canal en la entrada del 1D, (d1 − d1i)/2: el b1 de Gülich.
    pub inlet_width: f64,
    /// Dónde está el borde de ataque sobre la tapa delantera y sobre el cubo,
    /// como fracción de la longitud de cada curva.
    pub edge_shroud: f64,
    pub edge_hub: f64,
    /// Los dos extremos del borde de ataque, `[z, r]`: el de la tapa y el del
    /// cubo.
    pub edge_points: [[f64; 2]; 2],
    /// El abombado aplicado a P1 (cerca de la tapa) y a P2 (cerca del cubo),
    /// en mm.
    pub edge_bow: [f64; 2],
    /// Inclinación resultante del borde con la radial, en grados: positiva si
    /// el cubo queda aguas abajo de la tapa.
    pub edge_angle: f64,
    /// Longitud del borde de ataque, de la tapa al cubo, medida sobre la curva.
    pub edge_length: f64,
    /// Área de entrada sobre la superficie real del borde, 2π·∫r ds a lo largo
    /// de la curva (π·(r_tapa + r_cubo)·longitud si es recta). Con el borde
    /// radial y el dH del 1D es la del 1D.
    pub edge_area: f64,
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

/// Un punto de control movible: P1, P2 o P3 de una tapa, o el extremo del
/// borde de ataque sobre ella.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlPoint {
    pub wall: Wall,
    /// 1, 2 o 3 para P1, P2 y P3; 0 para el extremo del borde de ataque.
    pub index: usize,
}

/// Lo que se escribe al mover un punto de control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Moved {
    /// P2 en `[z, r]`, dentro del rectángulo de los extremos.
    Point([f64; 2]),
    /// P1 o P3: ángulo en grados y longitud en mm del brazo.
    Arm { angle: f64, length: f64 },
    /// El extremo del borde de ataque: fracción de la longitud de la curva
    /// desde el plano de entrada, dentro de [`EDGE_FRACTION_RANGE`].
    Fraction(f64),
}

/// Un punto de control y, si es de un brazo, el brazo que lo une a su extremo.
#[derive(Debug, Clone, PartialEq)]
pub struct Handle {
    pub control: ControlPoint,
    pub at: [f64; 2],
    /// Del extremo al punto: de P0 a P1 y de P4 a P3. P2 no tiene.
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
        // Solo el canal a partir del borde de ataque: las curvas siguen aguas
        // arriba de él.
        let front = cut_from(&front.points, self.params.edge_shroud);
        let rear = cut_from(&rear.points, self.params.edge_hub);
        let front_length = cumulative(&front);
        let rear_length = cumulative(&rear);
        let (front_total, rear_total) = (
            front_length[front_length.len() - 1],
            rear_length[rear_length.len() - 1],
        );
        let mut stations: Vec<ChannelStation> = Vec::with_capacity(front.len());
        let mut previous: Option<[f64; 2]> = None;
        for (point, length) in front.iter().zip(&front_length) {
            let partner = at_length(&rear, &rear_length, length / front_total * rear_total);
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
                z: middle[0],
                shroud_point: *point,
                hub_point: partner,
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
                    1 => Some((points[0], points[1])),
                    3 => Some((points[4], points[3])),
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

    /// Los dos extremos del borde de ataque, sobre la tapa y sobre el cubo.
    #[must_use]
    pub fn edge_handles(&self) -> Vec<Handle> {
        Wall::ALL
            .iter()
            .zip(self.params.edge_points)
            .map(|(wall, at)| Handle {
                control: ControlPoint {
                    wall: *wall,
                    index: 0,
                },
                at,
                line: None,
            })
            .collect()
    }

    /// Dónde está un punto de control, sea de una Bézier o del borde.
    #[must_use]
    pub fn control_position(&self, control: ControlPoint) -> Option<[f64; 2]> {
        self.handles()
            .into_iter()
            .chain(self.edge_handles())
            .find(|handle| handle.control == control)
            .map(|handle| handle.at)
    }

    /// Lo que se escribe si el punto de control se lleva a `pointer`: P2 va
    /// donde esté el puntero, recortado a su rectángulo; P1 y P3, al ángulo y
    /// la distancia del puntero a su extremo, recortados a [`ANGLE_RANGE`] y
    /// [`MIN_ARM`].
    #[must_use]
    pub fn moved(&self, control: ControlPoint, pointer: [f64; 2]) -> Moved {
        let shape = self.params.wall(control.wall);
        if control.index == 0 {
            let line = self
                .curve(control.wall.name())
                .map_or(&[][..], |curve| &curve.points[..]);
            return Moved::Fraction(
                arc_fraction(line, pointer).clamp(EDGE_FRACTION_RANGE.0, EDGE_FRACTION_RANGE.1),
            );
        }
        let (from, sign) = match control.index {
            1 => (shape.points[0], 1.0),
            3 => (shape.points[4], -1.0),
            _ => {
                let (low, high) = shape.p2_range;
                // La interfaz escribe el punto con 0,01 mm: se ajusta a esa
                // rejilla hacia dentro del rectángulo, para que el redondeo
                // del texto no lo saque (41,646 → «41,65» sería inválido).
                let inward = |axis: usize| {
                    let (lo, hi) = (
                        (low[axis] * 100.0).ceil() / 100.0,
                        (high[axis] * 100.0).floor() / 100.0,
                    );
                    let value =
                        (pointer[axis].clamp(low[axis], high[axis]) * 100.0).round() / 100.0;
                    if lo <= hi {
                        value.clamp(lo, hi)
                    } else {
                        pointer[axis].clamp(low[axis], high[axis])
                    }
                };
                return Moved::Point([inward(0), inward(1)]);
            }
        };
        // P1 está a +brazo desde P0; P3, a −brazo desde P4 (se avanza hacia P4).
        let (dz, dr) = (sign * (pointer[0] - from[0]), sign * (pointer[1] - from[1]));
        Moved::Arm {
            angle: dr
                .atan2(dz)
                .to_degrees()
                .clamp(ANGLE_RANGE.0, ANGLE_RANGE.1),
            length: dz.hypot(dr).max(MIN_ARM),
        }
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
/// entrada y radial a la salida (εDs = εTs = 0, p. 417), y las longitudes de
/// los brazos que hacen cada tapa lo más suave posible (mayor radio mínimo).
/// Gülich pide RDs = 0,6 a 0,8·b1 (p. 416), pero con zE de la ec. (7.2) un
/// giro de 90° de ese radio a menudo no cabe; la p. 417 admite radios
/// menores en bombas pequeñas. Es elección de hmflow.
///
/// Errores: el área de entrada no cabe en el ojo, un ángulo fuera de ±90° o
/// una longitud no positiva, un P2 que no es finito o que sale del rectángulo
/// de los extremos, tapas que se cruzan o un cubo que toca el eje.
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
    let auto_d1i = hub_squared.sqrt();
    let inlet_width = (d1 - auto_d1i) / 2.0;
    let d1i = edits.hub_diameter.unwrap_or(auto_d1i);
    if !(d1i.is_finite() && d1i >= 0.0 && d1i < d1) {
        return Err(DesignError::Input(format!(
            "meridiano: el diámetro del cubo dH vale {d1i} mm y tiene que estar entre 0 y d1 = {d1:.2} mm"
        )));
    }
    let fraction = |value: Option<f64>, what: &str| {
        let value = value.unwrap_or(0.0);
        if value.is_finite() && (EDGE_FRACTION_RANGE.0..=EDGE_FRACTION_RANGE.1).contains(&value) {
            Ok(value)
        } else {
            Err(DesignError::Input(format!(
                "meridiano: la posición del borde de ataque {what} vale {value} y tiene que estar entre {} y {}",
                EDGE_FRACTION_RANGE.0, EDGE_FRACTION_RANGE.1
            )))
        }
    };
    let edge_shroud = fraction(edits.edge_shroud, "sobre la tapa delantera")?;
    let edge_hub = fraction(edits.edge_hub, "sobre el cubo")?;
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
    // Con dH = 0 el cubo va sobre el eje: se admite hasta el redondeo.
    if rear.iter().any(|point| point[1] < -RANGE_TOLERANCE) {
        return Err(DesignError::Input(
            "meridiano: la tapa trasera cruza el eje".to_string(),
        ));
    }
    if crosses(&front, &rear) {
        return Err(DesignError::Input(
            "meridiano: las tapas delantera y trasera se cruzan".to_string(),
        ));
    }

    // El borde de ataque se apoya sobre las dos curvas, que siguen aguas arriba.
    let on_curve = |line: &[[f64; 2]], fraction: f64| {
        let lengths = cumulative(line);
        at_length(line, &lengths, fraction * lengths[lengths.len() - 1])
    };
    let edge_points = [on_curve(&front, edge_shroud), on_curve(&rear, edge_hub)];
    let (shroud_edge, hub_edge) = (edge_points[0], edge_points[1]);
    let bow = |value: Option<f64>, what: &str| match value {
        Some(value) if !value.is_finite() => Err(DesignError::Input(format!(
            "meridiano: el abombado del borde de ataque {what} no es un número"
        ))),
        Some(value) => Ok(value),
        None => Ok(0.0),
    };
    let edge_bow = [
        bow(edits.edge_bow_shroud, "cerca de la tapa")?,
        bow(edits.edge_bow_hub, "cerca del cubo")?,
    ];
    let chord = (hub_edge[0] - shroud_edge[0]).hypot(hub_edge[1] - shroud_edge[1]);
    let edge_angle = (hub_edge[0] - shroud_edge[0])
        .atan2(shroud_edge[1] - hub_edge[1])
        .to_degrees();
    // Recto (y con la cuerda nula no hay perpendicular): dos puntos, como
    // siempre. Curvo: la Bézier cúbica muestreada.
    let edge_line = if (edge_bow == [0.0, 0.0]) || chord < RANGE_TOLERANCE {
        edge_points.to_vec()
    } else {
        edge_bezier(shroud_edge, hub_edge, edge_bow)
    };
    let edge_length = cumulative(&edge_line)[edge_line.len() - 1];
    let edge_area = edge_line
        .windows(2)
        .map(|pair| {
            std::f64::consts::PI
                * (pair[0][1] + pair[1][1])
                * (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1])
        })
        .sum();

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
            points: edge_line,
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
            auto_d1i,
            inlet_width,
            edge_shroud,
            edge_hub,
            edge_points,
            edge_bow,
            edge_angle,
            edge_length,
            edge_area,
            z_e,
            r_ds,
            shroud,
            hub,
        },
    })
}

/// Tramos con que se muestrea la Bézier del borde de ataque.
const EDGE_SEGMENTS: usize = 24;

/// El borde de ataque como Bézier cúbica de la tapa al cubo: P1 y P2 en el
/// tercio y los dos tercios de la cuerda, desplazados `bow` mm en perpendicular
/// (positivo hacia aguas arriba: la normal es (Δr, −Δz)/|cuerda|, que en un
/// borde radial apunta a −z).
fn edge_bezier(from: [f64; 2], to: [f64; 2], bow: [f64; 2]) -> Vec<[f64; 2]> {
    let (dz, dr) = (to[0] - from[0], to[1] - from[1]);
    let chord = dz.hypot(dr);
    let normal = [dr / chord, -dz / chord];
    let at = |fraction: f64, offset: f64| {
        [
            from[0] + fraction * dz + offset * normal[0],
            from[1] + fraction * dr + offset * normal[1],
        ]
    };
    let (p1, p2) = (at(1.0 / 3.0, bow[0]), at(2.0 / 3.0, bow[1]));
    (0..=EDGE_SEGMENTS)
        .map(|step| {
            let t = step as f64 / EDGE_SEGMENTS as f64;
            let u = 1.0 - t;
            let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let points = [from, p1, p2, to];
            [
                (0..4).map(|k| weights[k] * points[k][0]).sum(),
                (0..4).map(|k| weights[k] * points[k][1]).sum(),
            ]
        })
        .collect()
}

/// Los 5 puntos con P1 a `p1` de P0 hacia P2 y P3 a `p3` de P4 hacia P2: solo
/// para buscar las longitudes más suaves con P2 en la esquina.
fn fraction_points(ends: ([f64; 2], [f64; 2]), p2: [f64; 2], p1: f64, p3: f64) -> [[f64; 2]; 5] {
    let (p0, p4) = ends;
    let toward = |from: [f64; 2], fraction: f64| {
        [
            from[0] + (p2[0] - from[0]) * fraction,
            from[1] + (p2[1] - from[1]) * fraction,
        ]
    };
    [p0, toward(p0, p1), p2, toward(p4, p3), p4]
}

/// Los 5 puntos con los brazos: P1 sale de P0 y P3 llega a P4 con su ángulo
/// (grados, desde +z hacia +r) y su longitud.
fn arm_points(
    ends: ([f64; 2], [f64; 2]),
    p2: [f64; 2],
    start: (f64, f64),
    end: (f64, f64),
) -> [[f64; 2]; 5] {
    let (p0, p4) = ends;
    let (start_angle, start_length) = (start.0.to_radians(), start.1);
    let (end_angle, end_length) = (end.0.to_radians(), end.1);
    [
        p0,
        [
            p0[0] + start_length * start_angle.cos(),
            p0[1] + start_length * start_angle.sin(),
        ],
        p2,
        [
            p4[0] - end_length * end_angle.cos(),
            p4[1] - end_length * end_angle.sin(),
        ],
        p4,
    ]
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

/// Radio de curvatura mínimo a lo largo de toda la curva y el punto donde
/// está. Una curva sin curvatura (recta) da infinito en P0.
fn min_radius(points: &[[f64; 2]; 5]) -> Result<(f64, [f64; 2]), DesignError> {
    let (radius, t) = min_radius_with_t(points, RADIUS_SAMPLES)?;
    let at = bezier(points)?.point_at(t).map_err(geometry_failed)?;
    Ok((radius, [at.x, at.y]))
}

impl WallShape {
    /// Curvatura con signo de la tapa frente a la fracción de su longitud
    /// desarrollada (M/Mmax), como `[fracción, κ en 1/mm]`. Muestrea la Bézier
    /// completa, con `samples` + 1 puntos.
    pub fn curvature_profile(&self, samples: usize) -> Result<Vec<[f64; 2]>, DesignError> {
        let curve = bezier(&self.points)?;
        let samples = samples.max(2);
        let mut length = 0.0;
        let mut previous: Option<[f64; 2]> = None;
        let mut rows = Vec::with_capacity(samples + 1);
        for index in 0..=samples {
            let t = index as f64 / samples as f64;
            let at = curve.point_at(t).map_err(geometry_failed)?;
            if let Some(last) = previous {
                length += (at.x - last[0]).hypot(at.y - last[1]);
            }
            previous = Some([at.x, at.y]);
            let curvature = curve.curvature_at(t).map_err(geometry_failed)?;
            rows.push([length, curvature]);
        }
        if length > 0.0 {
            for row in &mut rows {
                row[0] /= length;
            }
        }
        Ok(rows)
    }
}

fn min_radius_sampled(points: &[[f64; 2]; 5], samples: usize) -> Result<f64, DesignError> {
    Ok(min_radius_with_t(points, samples)?.0)
}

fn min_radius_with_t(points: &[[f64; 2]; 5], samples: usize) -> Result<(f64, f64), DesignError> {
    let curve = bezier(points)?;
    let mut radius = f64::INFINITY;
    let mut at = 0.0;
    // Con los brazos inclinados la curvatura de los extremos no es nula: cuenta.
    for index in 0..=samples {
        let t = index as f64 / samples as f64;
        let curvature = curve.curvature_at(t).map_err(geometry_failed)?.abs();
        if curvature > 0.0 && 1.0 / curvature < radius {
            radius = 1.0 / curvature;
            at = t;
        }
    }
    Ok((radius, at))
}

/// Las fracciones de P1 y P3 que hacen la curva más suave: las que dan el
/// mayor radio de curvatura mínimo. Con P1 y P3 pegados a la esquina la curva
/// se dobla en punta; pegados a los extremos, la velocidad se anula allí y la
/// curvatura se dispara. Búsqueda por coordenadas con pasos que se achican.
fn smoothest_fractions(
    ends: ([f64; 2], [f64; 2]),
    corner: [f64; 2],
) -> Result<(f64, f64), DesignError> {
    let radius = |p1: f64, p3: f64| {
        min_radius_sampled(&fraction_points(ends, corner, p1, p3), SEARCH_SAMPLES)
    };
    let (low, high) = SEARCH_FRACTIONS;
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
    let name = wall.name().to_lowercase();
    let (fraction_1, fraction_3) = smoothest_fractions(ends, corner)?;
    let distance = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let auto_start_length = fraction_1 * distance(ends.0, corner);
    let auto_end_length = fraction_3 * distance(ends.1, corner);

    let low = [ends.0[0].min(ends.1[0]), ends.0[1].min(ends.1[1])];
    let high = [ends.0[0].max(ends.1[0]), ends.0[1].max(ends.1[1])];
    let p2 = edit.p2.unwrap_or(corner);
    if !(p2[0].is_finite() && p2[1].is_finite()) {
        return Err(DesignError::Input(format!(
            "meridiano: P2 de la {name} no es un punto"
        )));
    }
    let inside = (0..2).all(|axis| {
        p2[axis] >= low[axis] - RANGE_TOLERANCE && p2[axis] <= high[axis] + RANGE_TOLERANCE
    });
    if !inside {
        return Err(DesignError::Input(format!(
            "meridiano: P2 de la {name} (z = {:.2}, r = {:.2} mm) queda fuera del rectángulo de los extremos: z entre {:.2} y {:.2} mm, r entre {:.2} y {:.2} mm",
            p2[0], p2[1], low[0], high[0], low[1], high[1]
        )));
    }

    let angle = |value: Option<f64>, auto: f64, what: &str| {
        let value = value.unwrap_or(auto);
        if value.is_finite() && (ANGLE_RANGE.0..=ANGLE_RANGE.1).contains(&value) {
            Ok(value)
        } else {
            Err(DesignError::Input(format!(
                "meridiano: {what} de la {name} vale {value} y tiene que estar entre {} y {} grados",
                ANGLE_RANGE.0, ANGLE_RANGE.1
            )))
        }
    };
    let length = |value: Option<f64>, auto: f64, what: &str| {
        let value = value.unwrap_or(auto);
        if value.is_finite() && value > 0.0 {
            Ok(value)
        } else {
            Err(DesignError::Input(format!(
                "meridiano: {what} de la {name} vale {value} y tiene que ser mayor que 0 mm"
            )))
        }
    };
    let start_angle = angle(edit.start_angle, AUTO_START_ANGLE, "ángulo de inicio")?;
    let end_angle = angle(edit.end_angle, AUTO_END_ANGLE, "ángulo final")?;
    let start_length = length(edit.start_length, auto_start_length, "longitud de inicio")?;
    let end_length = length(edit.end_length, auto_end_length, "longitud final")?;

    let points = arm_points(
        ends,
        p2,
        (start_angle, start_length),
        (end_angle, end_length),
    );
    let (min_radius, max_curvature_at) = min_radius(&points)?;
    Ok(WallShape {
        points,
        start_angle,
        start_length,
        end_angle,
        end_length,
        auto_start_length,
        auto_end_length,
        p2_range: (low, high),
        min_radius,
        max_curvature_at,
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
    /// Posición axial de la línea media.
    pub z: f64,
    /// Los dos puntos emparejados de la estación, `[z, r]`: el de la tapa
    /// delantera y el de la trasera. La línea media es su punto medio.
    pub shroud_point: [f64; 2],
    pub hub_point: [f64; 2],
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

/// La fracción de la longitud de la polilínea a la que cae el punto de ella
/// más cercano a `target`.
fn arc_fraction(line: &[[f64; 2]], target: [f64; 2]) -> f64 {
    if line.len() < 2 {
        return 0.0;
    }
    let lengths = cumulative(line);
    let total = lengths[lengths.len() - 1];
    let mut best = (f64::INFINITY, 0.0);
    for (index, pair) in line.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let span = dx * dx + dy * dy;
        let t = if span > 0.0 {
            (((target[0] - a[0]) * dx + (target[1] - a[1]) * dy) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let distance = (a[0] + t * dx - target[0]).hypot(a[1] + t * dy - target[1]);
        if distance < best.0 {
            best = (distance, lengths[index] + t * span.sqrt());
        }
    }
    if total > 0.0 { best.1 / total } else { 0.0 }
}

/// La polilínea desde la fracción `fraction` de su longitud: el punto exacto
/// de partida y los vértices que le siguen.
fn cut_from(line: &[[f64; 2]], fraction: f64) -> Vec<[f64; 2]> {
    let lengths = cumulative(line);
    let start = fraction * lengths[lengths.len() - 1];
    let mut cut = vec![at_length(line, &lengths, start)];
    for (point, length) in line.iter().zip(&lengths) {
        if *length > start {
            cut.push(*point);
        }
    }
    cut
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
