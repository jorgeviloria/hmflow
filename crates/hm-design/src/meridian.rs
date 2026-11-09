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

/// Lo editado del borde de ataque, una Bézier de 5 puntos de la tapa (P0) al
/// cubo (P4). Se mide en el marco de la cuerda, no en coordenadas del corte: el
/// eje es la cuerda de P0 a P4 y la normal, perpendicular a ella, positiva hacia
/// aguas arriba (en un borde radial, hacia −z). Así la curva sigue a los
/// extremos cuando se arrastran. Todo `None` es el borde automático: la Bézier
/// ajustada al borde que sigue el canal (cada línea de corriente corta el borde
/// a una fracción de pared interpolada entre la del cubo y la de la tapa).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EdgeEdit {
    /// P2 a lo largo de la cuerda, de 0 (en P0) a 1 (en P4). `None` es el del ajuste al borde que sigue el canal.
    pub p2_along: Option<f64>,
    /// P2 separado de la cuerda, en mm, con signo. `None` es 0.
    pub p2_offset: Option<f64>,
    /// Ángulo del brazo de inicio (P0→P1) con la cuerda, en grados dentro de
    /// [`ANGLE_RANGE`]; positivo gira hacia aguas arriba. `None` es el del
    /// ajuste al borde que sigue el canal.
    pub start_angle: Option<f64>,
    /// Longitud del brazo de inicio, en mm, mayor que 0. `None` es la del
    /// ajuste al borde que sigue el canal.
    pub start_length: Option<f64>,
    /// Ángulo del brazo final (P4→P3) con la cuerda vista desde P4, igual;
    /// `None` es el del ajuste al borde que sigue el canal.
    pub end_angle: Option<f64>,
    pub end_length: Option<f64>,
}

impl EdgeEdit {
    /// Sin nada editado: el borde automático, perpendicular a las dos paredes.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Nombre de la curva del recorte del cubo.
pub const CUT_NAME: &str = "Corte del cubo";

/// Cómo termina el recorte del cubo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CutMode {
    /// La cara del recorte baja hasta el eje: cubo cerrado.
    #[default]
    Closed,
    /// Agujero pasante para el eje, con avellanado y chaflán opcionales.
    Through,
}

/// El recorte del cubo editado. Es geometría mecánica: no entra en nada
/// hidráulico.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HubCutEdit {
    /// z de la cara del recorte, en mm. `None`: sin recorte.
    pub z: Option<f64>,
    pub mode: CutMode,
    /// Diámetro del agujero del eje, en mm (solo pasante).
    pub shaft_diameter: Option<f64>,
    /// Diámetro y profundidad del avellanado, en mm (solo pasante).
    pub counterbore_diameter: Option<f64>,
    pub counterbore_depth: Option<f64>,
    /// Chaflán a 45° del agujero, en mm (solo pasante).
    pub chamfer: Option<f64>,
}

impl HubCutEdit {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.z.is_some()
    }
}

/// Lo editado del corte.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MeridianEdits {
    /// Diámetro del cubo en el plano de entrada, dH, en mm: de 0 (el cubo
    /// arranca en el eje) a menos que d1. `None` es el que da el área del 1D.
    pub hub_diameter: Option<f64>,
    /// Posición del borde de ataque sobre la tapa delantera y sobre el cubo,
    /// como fracción de la longitud de cada curva desde el plano de entrada,
    /// dentro de [`EDGE_FRACTION_RANGE`]. En la tapa, `None` es 0 (el ojo); en
    /// el cubo, `None` es la fracción que iguala el momento estático S de las
    /// dos paredes (criterio S).
    pub edge_shroud: Option<f64>,
    pub edge_hub: Option<f64>,
    /// El borde de ataque como Bézier de 5 puntos, de la tapa al cubo.
    pub edge: EdgeEdit,
    pub shroud: WallEdit,
    pub hub: WallEdit,
    /// Recorte mecánico del cubo, sin efecto en el perfil hidráulico.
    pub hub_cut: HubCutEdit,
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

/// El borde de ataque como Bézier de 5 puntos, con lo aplicado (en mm y grados).
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeShape {
    /// Los 5 puntos de la Bézier, de P0 (en la tapa) a P4 (en el cubo).
    pub points: [[f64; 2]; 5],
    /// Longitud de la cuerda de P0 a P4. Con 0 no hay marco: no se edita.
    pub chord: f64,
    /// P2 en el marco de la cuerda: a lo largo (0 a 1) y separado (mm).
    pub p2_along: f64,
    pub p2_offset: f64,
    pub start_angle: f64,
    pub start_length: f64,
    pub end_angle: f64,
    pub end_length: f64,
    /// Los valores automáticos: los del ajuste al borde que sigue el canal.
    pub auto: EdgeFit,
}

/// Los seis valores de la Bézier del borde en el marco de su cuerda (mm y
/// grados): lo que sale de ajustarla al borde que sigue el canal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeFit {
    pub p2_along: f64,
    pub p2_offset: f64,
    pub start_angle: f64,
    pub start_length: f64,
    pub end_angle: f64,
    pub end_length: f64,
}

/// El recorte del cubo usado: la cara en `z`, hasta `hub_radius` (el radio de
/// la tapa trasera en esa z), y el contorno `[z, r]` hacia el interior.
#[derive(Debug, Clone, PartialEq)]
pub struct HubCutShape {
    pub z: f64,
    pub hub_radius: f64,
    pub mode: CutMode,
    /// Radio del agujero del eje; 0 si el cubo es cerrado.
    pub shaft_radius: f64,
    pub points: Vec<[f64; 2]>,
}

/// Por qué el borde de ataque automático no iguala los momentos estáticos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeClamp {
    /// Con la fracción 0 el cubo ya tiene menos momento estático que la tapa.
    AtInlet,
    /// Igualarlos pide pasar del límite de [`EDGE_FRACTION_RANGE`].
    AtLimit,
}

/// Lo que dio el criterio S para el borde de ataque.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeAuto {
    /// La fracción del cubo salió del criterio S; `false` si estaba escrita.
    pub hub_auto: bool,
    /// Momento estático S = ∫ r ds, en mm², de la tapa delantera y del cubo,
    /// desde los extremos del borde usados hasta la salida.
    pub s_shroud: f64,
    pub s_hub: f64,
    /// `Some` si el criterio se quedó en un límite sin igualar los momentos.
    pub clamp: Option<EdgeClamp>,
}

/// Las cotas con que se construyó el corte, en mm.
#[derive(Debug, Clone, PartialEq)]
pub struct MeridianParams {
    /// Diámetro del cubo en el plano de entrada, dH: el editado o, si no, el que
    /// da el área del 1D con el borde radial, π·(d1² − d1i²)/4 = π·d1·b1.
    pub d1i: f64,
    /// El d1i del 1D, π·(d1² − d1i²)/4 = π·d1·b1: referencia de b1g, z_e y r_ds.
    pub auto_d1i: f64,
    /// Anchura del canal en la entrada del 1D, (d1 − d1i)/2: el b1 de Gülich.
    pub inlet_width: f64,
    /// Dónde está el borde de ataque sobre la tapa delantera y sobre el cubo,
    /// como fracción de la longitud de cada curva.
    pub edge_shroud: f64,
    pub edge_hub: f64,
    /// El criterio S del borde de ataque: si la fracción del cubo fue
    /// automática, los dos S y el límite alcanzado, si lo hubo.
    pub edge_auto: EdgeAuto,
    /// Los dos extremos del borde de ataque, `[z, r]`: el de la tapa y el del
    /// cubo.
    pub edge_points: [[f64; 2]; 2],
    /// La Bézier del borde de ataque con lo aplicado.
    pub edge: EdgeShape,
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
    /// El recorte del cubo; `None` sin recorte.
    pub hub_cut: Option<HubCutShape>,
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

/// Un punto de control movible: P1, P2 o P3 de una tapa, el extremo del borde
/// de ataque sobre ella, P1, P2 o P3 del propio borde o el punto del recorte
/// del cubo ([`CUT_CONTROL`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlPoint {
    pub wall: Wall,
    /// 1, 2 o 3 para P1, P2 y P3; 0 para el extremo del borde de ataque; 5, 6
    /// y 7 para P1, P2 y P3 de la Bézier del borde (con la tapa delantera, que
    /// no cuenta); 8 con la tapa trasera es el recorte del cubo.
    pub index: usize,
}

/// El punto de control reservado del recorte del cubo: solo se mueve en z y
/// no es una `Handle` de las tapas ni del borde.
pub const CUT_CONTROL: ControlPoint = ControlPoint {
    wall: Wall::Hub,
    index: 8,
};

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
    /// P2 del borde de ataque en el marco de la cuerda: a lo largo (0 a 1) y
    /// separado de ella (mm, con signo).
    EdgePoint { along: f64, offset: f64 },
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
    /// El punto arrastrable del recorte, `[z, hub_radius]`.
    #[must_use]
    pub fn cut_handle(&self) -> Option<[f64; 2]> {
        self.params
            .hub_cut
            .as_ref()
            .map(|cut| [cut.z, cut.hub_radius])
    }

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
        // Solo el canal a partir del borde de ataque: las curvas siguen aguas
        // arriba de él.
        let mut stations = self.channel_from(self.params.edge_shroud, self.params.edge_hub);
        // Con el borde curvo, la primera estación es el borde mismo: su área es
        // la de la superficie, 2π·∫r ds sobre la curva, no la de la cuerda. Con
        // el borde recto son la misma y no se toca.
        let curved = self
            .curve("Borde de ataque")
            .is_some_and(|edge| edge.points.len() > 2);
        if let (true, Some(first)) = (curved, stations.first_mut()) {
            first.area = self.params.edge_area;
        }
        stations
    }

    /// Lo mismo que [`Self::channel`] pero a partir de esas fracciones de la
    /// longitud de la tapa delantera y del cubo. Con (0, 0) es el canal entero,
    /// desde el plano de entrada, donde arrancan las dos curvas.
    #[must_use]
    pub fn channel_from(&self, shroud: f64, hub: f64) -> Vec<ChannelStation> {
        let (Some(front), Some(rear)) = (self.curve("Tapa delantera"), self.curve("Tapa trasera"))
        else {
            return Vec::new();
        };
        let front = cut_from(&front.points, shroud);
        let rear = cut_from(&rear.points, hub);
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

    /// Los puntos movibles del borde de ataque: sus dos extremos, sobre la tapa
    /// y sobre el cubo, y P1, P2 y P3 de su Bézier (P1 y P3 con el brazo que los
    /// une a su extremo). Sin cuerda (extremos juntos) no hay marco y no hay
    /// puntos interiores.
    #[must_use]
    pub fn edge_handles(&self) -> Vec<Handle> {
        let ends = self.params.edge_points;
        let mut handles: Vec<Handle> = Wall::ALL
            .iter()
            .zip(ends)
            .map(|(wall, at)| Handle {
                control: ControlPoint {
                    wall: *wall,
                    index: 0,
                },
                at,
                line: None,
            })
            .collect();
        let edge = &self.params.edge;
        if edge.chord >= RANGE_TOLERANCE {
            for (index, point) in [(5, 1), (6, 2), (7, 3)] {
                let arm = match index {
                    5 => Some((edge.points[0], edge.points[1])),
                    7 => Some((edge.points[4], edge.points[3])),
                    _ => None,
                };
                handles.push(Handle {
                    control: ControlPoint {
                        wall: Wall::Shroud,
                        index,
                    },
                    at: edge.points[point],
                    line: arm,
                });
            }
        }
        handles
    }

    /// Dónde está un punto de control, sea de una Bézier o del borde.
    #[must_use]
    pub fn control_position(&self, control: ControlPoint) -> Option<[f64; 2]> {
        if control == CUT_CONTROL {
            return self.cut_handle();
        }
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
        if control.index >= 5 {
            let edge = &self.params.edge;
            let Some((u, n)) = edge_frame(edge.points[0], edge.points[4]) else {
                return Moved::EdgePoint {
                    along: 0.5,
                    offset: 0.0,
                };
            };
            let rounded = |value: f64, scale: f64| (value * scale).round() / scale;
            let relative = |origin: [f64; 2]| {
                let v = [pointer[0] - origin[0], pointer[1] - origin[1]];
                (v[0] * u[0] + v[1] * u[1], v[0] * n[0] + v[1] * n[1])
            };
            let wanted = match control.index {
                5 | 7 => {
                    // P1 se mide desde P0 a lo largo de la cuerda; P3, desde P4
                    // hacia P0.
                    let (origin, sign) = if control.index == 5 {
                        (edge.points[0], 1.0)
                    } else {
                        (edge.points[4], -1.0)
                    };
                    let (along, across) = relative(origin);
                    Moved::Arm {
                        angle: rounded(
                            across
                                .atan2(sign * along)
                                .to_degrees()
                                .clamp(ANGLE_RANGE.0, ANGLE_RANGE.1),
                            10.0,
                        ),
                        length: rounded(along.hypot(across).max(MIN_ARM), 100.0),
                    }
                }
                _ => {
                    let (along, across) = relative(edge.points[0]);
                    Moved::EdgePoint {
                        along: rounded((along / edge.chord).clamp(0.0, 1.0), 1000.0),
                        offset: rounded(across, 100.0),
                    }
                }
            };
            return self.limit_to_channel(control.index, wanted);
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

    /// El contorno del canal para decidir si el borde de ataque cabe: la tapa
    /// delantera, el cubo al revés, y ambas prolongadas aguas arriba, donde el
    /// borde puede entrar (su desvío positivo va hacia allí).
    fn channel_outline(&self) -> Option<Vec<[f64; 2]>> {
        let shroud = &self.curve(Wall::Shroud.name())?.points;
        let hub = &self.curve(Wall::Hub.name())?.points;
        let (shroud_start, hub_start) = (*shroud.first()?, *hub.first()?);
        let mut outline = vec![[shroud_start[0] - UPSTREAM_EXTENSION_MM, shroud_start[1]]];
        outline.extend(shroud.iter().copied());
        outline.extend(hub.iter().rev().copied());
        outline.push([hub_start[0] - UPSTREAM_EXTENSION_MM, hub_start[1]]);
        Some(outline)
    }

    /// Si los puntos del borde de ataque, salvo sus dos extremos (que están
    /// sobre las tapas), quedan dentro del canal. Una décima de rebase se
    /// perdona: el apoyo en la curva y el redondeo de las cajas.
    #[must_use]
    pub fn edge_inside_channel(&self, points: &[[f64; 2]; 5]) -> bool {
        let Some(outline) = self.channel_outline() else {
            return true;
        };
        let Ok(samples) = sample(points) else {
            return true;
        };
        let inner = &samples[1..samples.len() - 1];
        inner.iter().all(|point| {
            point_in_polygon(&outline, *point)
                || distance_to_closed_line(&outline, *point) <= CHANNEL_TOLERANCE_MM
        })
    }

    /// El borde con `moved` aplicado al punto `index` (5, 6 o 7), si cabe en el
    /// canal. Un valor que no se puede construir no cabe.
    fn edge_fits(&self, index: usize, moved: Moved) -> bool {
        let edge = &self.params.edge;
        let mut edit = EdgeEdit {
            p2_along: Some(edge.p2_along),
            p2_offset: Some(edge.p2_offset),
            start_angle: Some(edge.start_angle),
            start_length: Some(edge.start_length),
            end_angle: Some(edge.end_angle),
            end_length: Some(edge.end_length),
        };
        match (index, moved) {
            (5, Moved::Arm { angle, length }) => {
                edit.start_angle = Some(angle);
                edit.start_length = Some(length);
            }
            (7, Moved::Arm { angle, length }) => {
                edit.end_angle = Some(angle);
                edit.end_length = Some(length);
            }
            (6, Moved::EdgePoint { along, offset }) => {
                edit.p2_along = Some(along);
                edit.p2_offset = Some(offset);
            }
            _ => return true,
        }
        // Todo está escrito en `edit`: los valores automáticos no se usan.
        edge_shape(edge.points[0], edge.points[4], &edit, &edge.auto)
            .is_ok_and(|shape| self.edge_inside_channel(&shape.points))
    }

    /// Lleva `wanted` hasta el último valor que deja el borde dentro del canal,
    /// buscándolo por bisección entre el valor actual y el pedido. Si el borde
    /// actual ya sale del canal no hay a dónde volver, y no se recorta: un
    /// archivo guardado así se puede seguir arrastrando.
    fn limit_to_channel(&self, index: usize, wanted: Moved) -> Moved {
        let edge = &self.params.edge;
        let current = match index {
            5 => Moved::Arm {
                angle: edge.start_angle,
                length: edge.start_length,
            },
            7 => Moved::Arm {
                angle: edge.end_angle,
                length: edge.end_length,
            },
            _ => Moved::EdgePoint {
                along: edge.p2_along,
                offset: edge.p2_offset,
            },
        };
        if self.edge_fits(index, wanted) || !self.edge_fits(index, current) {
            return wanted;
        }
        let (mut inside, mut outside) = (0.0, 1.0);
        for _ in 0..24 {
            let middle = 0.5 * (inside + outside);
            if self.edge_fits(index, blend(current, wanted, middle)) {
                inside = middle;
            } else {
                outside = middle;
            }
        }
        let limited = rounded_move(blend(current, wanted, inside));
        if self.edge_fits(index, limited) {
            limited
        } else {
            current
        }
    }

    /// El ángulo del brazo de P1 (`index` 5) o de P3 (7), con la cuerda y en
    /// grados con una décima, con que sale perpendicular a su pared, hacia el
    /// canal. `None` para otro punto, o sin cuerda.
    #[must_use]
    pub fn perpendicular_angle(&self, index: usize) -> Option<f64> {
        let edge = &self.params.edge;
        let (wall, end, other, sign) = match index {
            5 => (Wall::Shroud, edge.points[0], edge.points[4], 1.0),
            7 => (Wall::Hub, edge.points[4], edge.points[0], -1.0),
            _ => return None,
        };
        perpendicular_arm_angle(&self.curve(wall.name())?.points, end, other, sign)
    }

    /// Avisos del corte: el borde de ataque automático que se queda en un
    /// límite sin igualar los momentos estáticos (`HM-DES-051`, informativo).
    #[must_use]
    pub fn diagnostics(&self) -> Vec<hm_core::Diagnostic> {
        let auto = &self.params.edge_auto;
        let Some(clamp) = auto.clamp else {
            return Vec::new();
        };
        let hint = match clamp {
            EdgeClamp::AtInlet => format!(
                "Con el borde del cubo en el plano de entrada, S cubo = {:.0} mm² ya queda por debajo de S tapa = {:.0} mm². Mueve el borde de la tapa aguas abajo o abre el cubo.",
                auto.s_hub, auto.s_shroud
            ),
            EdgeClamp::AtLimit => format!(
                "Igualar S pide pasar del {:.0} % del cubo: S cubo = {:.0} mm² frente a S tapa = {:.0} mm². Acerca el borde de la tapa al ojo o cierra el cubo.",
                EDGE_FRACTION_RANGE.1 * 100.0,
                auto.s_hub,
                auto.s_shroud
            ),
        };
        vec![
            hm_core::Diagnostic::info(
                "HM-DES-051",
                "El borde de ataque automático no iguala el momento estático de cubo y tapa: se queda en el límite.",
            )
            .with_hint(hint),
        ]
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
    // El perfil es el de las fórmulas: sin dH escrito, el del 1D. Llevar el
    // cubo al eje es geometría mecánica y se hace encima, con los segmentos.
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
    let written_hub = edits
        .edge_hub
        .map(|value| fraction(Some(value), "sobre el cubo"))
        .transpose()?;
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

    // Borde de ataque: la tapa en su fracción (0 es el ojo) y el cubo donde su
    // momento estático iguala al de la tapa, salvo que esté escrito.
    let (edge_hub, edge_auto) = edge_hub_fraction(&front, &rear, edge_shroud, written_hub);

    // El borde de ataque se apoya sobre las dos curvas, que siguen aguas arriba.
    let on_curve = |line: &[[f64; 2]], fraction: f64| {
        let lengths = cumulative(line);
        at_length(line, &lengths, fraction * lengths[lengths.len() - 1])
    };
    let edge_points = [on_curve(&front, edge_shroud), on_curve(&rear, edge_hub)];
    let (shroud_edge, hub_edge) = (edge_points[0], edge_points[1]);
    // Sin nada escrito, la Bézier es el ajuste al borde que sigue el canal.
    let channel = channel_edge(&front, &rear, edge_shroud, edge_hub, CURVE_POINTS);
    let fit = fit_edge(shroud_edge, hub_edge, &channel);
    let edge = edge_shape(shroud_edge, hub_edge, &edits.edge, &fit)?;
    let edge_angle = (hub_edge[0] - shroud_edge[0])
        .atan2(shroud_edge[1] - hub_edge[1])
        .to_degrees();
    // Sin cuerda (extremos coincidentes): los dos puntos. Si no, la Bézier de 5
    // puntos muestreada, editada o automática.
    let edge_line = if edge.chord < RANGE_TOLERANCE {
        edge_points.to_vec()
    } else {
        sample(&edge.points)?
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

    // El recorte es mecánico: se calcula aparte y no vuelve a lo hidráulico.
    let hub_cut = hub_cut_shape(&rear, edge_points[1][0], &edits.hub_cut)?;

    let mut curves = vec![
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
    if let Some(cut) = &hub_cut {
        curves.push(PlanCurve {
            name: CUT_NAME.to_string(),
            points: cut.points.clone(),
        });
    }
    Ok(MeridionalModel {
        curves,
        params: MeridianParams {
            d1i,
            auto_d1i,
            inlet_width,
            edge_shroud,
            edge_hub,
            edge_auto,
            edge_points,
            edge,
            edge_angle,
            edge_length,
            edge_area,
            z_e,
            r_ds,
            shroud,
            hub,
            hub_cut,
        },
    })
}

/// El contorno del recorte del cubo sobre la tapa trasera `rear`. `None` sin
/// recorte. No toca el perfil hidráulico.
fn hub_cut_shape(
    rear: &[[f64; 2]],
    edge_hub_z: f64,
    edit: &HubCutEdit,
) -> Result<Option<HubCutShape>, DesignError> {
    let Some(zc) = edit.z else {
        return Ok(None);
    };
    let z_end = rear[rear.len() - 1][0];
    if !(zc.is_finite() && zc >= 0.0 && zc <= z_end) {
        return Err(DesignError::Input(format!(
            "recorte del cubo: z vale {zc} mm y tiene que estar entre 0 y {z_end:.2} mm, el final del cubo"
        )));
    }
    if zc > edge_hub_z {
        return Err(DesignError::Input(format!(
            "recorte del cubo: z = {zc} mm pasa del borde de ataque: el z máximo ahora es {edge_hub_z:.2} mm (el borde de ataque sobre el cubo); si es muy pequeño, mueve antes «Borde en cubo» aguas abajo"
        )));
    }
    let rc = rear
        .windows(2)
        .find(|pair| pair[0][0] <= zc && zc <= pair[1][0])
        .map_or(rear[0][1], |pair| {
            let span = pair[1][0] - pair[0][0];
            if span > 0.0 {
                pair[0][1] + (zc - pair[0][0]) / span * (pair[1][1] - pair[0][1])
            } else {
                pair[0][1]
            }
        });

    if edit.mode == CutMode::Closed {
        return Ok(Some(HubCutShape {
            z: zc,
            hub_radius: rc,
            mode: CutMode::Closed,
            shaft_radius: 0.0,
            points: vec![[zc, rc], [zc, 0.0]],
        }));
    }

    let Some(shaft_diameter) = edit.shaft_diameter else {
        return Err(DesignError::Input(
            "recorte del cubo: el recorte pasante necesita el diámetro del eje, y no hay"
                .to_string(),
        ));
    };
    let rs = shaft_diameter / 2.0;
    if !(rs.is_finite() && rs > 0.0 && rs < rc) {
        return Err(DesignError::Input(format!(
            "recorte del cubo: el diámetro del eje vale {shaft_diameter} mm y su radio tiene que estar entre 0 y el radio del cubo en z = {zc} mm, {rc:.2} mm"
        )));
    }
    let mut points = vec![[zc, rc]];
    // z y radio hasta donde llega el fondo del avellanado (o la cara).
    let (z0, widest) = if let Some(diameter) = edit.counterbore_diameter {
        let rb = diameter / 2.0;
        if !(rb.is_finite() && rb > rs && rb < rc) {
            return Err(DesignError::Input(format!(
                "recorte del cubo: el diámetro del avellanado vale {diameter} mm y su radio tiene que estar entre el del eje, {rs:.2} mm, y el del cubo, {rc:.2} mm"
            )));
        }
        let depth = edit.counterbore_depth.unwrap_or(0.0);
        if !(depth.is_finite() && depth > 0.0) {
            return Err(DesignError::Input(format!(
                "recorte del cubo: la profundidad del avellanado vale {depth} mm y tiene que ser positiva (diámetro {diameter} mm)"
            )));
        }
        if zc + depth > z_end {
            return Err(DesignError::Input(format!(
                "recorte del cubo: la cara en z = {zc} mm más el avellanado de {depth} mm pasa del final del cubo, z = {z_end:.2} mm"
            )));
        }
        points.extend([[zc, rb], [zc + depth, rb]]);
        (zc + depth, rb)
    } else {
        (zc, rc)
    };
    match edit.chamfer {
        Some(chamfer) => {
            if !(chamfer.is_finite() && chamfer > 0.0 && chamfer <= widest - rs) {
                return Err(DesignError::Input(format!(
                    "recorte del cubo: el chaflán vale {chamfer} mm y tiene que ser positivo y no mayor que {:.2} mm, la diferencia entre el radio {widest:.2} mm y el del eje {rs:.2} mm",
                    widest - rs
                )));
            }
            if z0 + chamfer > z_end {
                return Err(DesignError::Input(format!(
                    "recorte del cubo: el chaflán de {chamfer} mm desde z = {z0} mm pasa del final del cubo, z = {z_end:.2} mm"
                )));
            }
            points.extend([[z0, rs + chamfer], [z0 + chamfer, rs]]);
        }
        None => points.push([z0, rs]),
    }
    points.push([z_end, rs]);
    Ok(Some(HubCutShape {
        z: zc,
        hub_radius: rc,
        mode: CutMode::Through,
        shaft_radius: rs,
        points,
    }))
}

/// Cuánto se prolongan las tapas aguas arriba para decidir si el borde de
/// ataque queda dentro del canal, en mm.
const UPSTREAM_EXTENSION_MM: f64 = 1000.0;

/// Lo que un punto del borde puede quedar fuera de una pared sin contar como
/// salida, en mm.
const CHANNEL_TOLERANCE_MM: f64 = 0.05;

/// Si `point` está dentro del polígono cerrado `outline` (regla par-impar).
fn point_in_polygon(outline: &[[f64; 2]], point: [f64; 2]) -> bool {
    let mut inside = false;
    let mut previous = outline[outline.len() - 1];
    for &current in outline {
        let crosses = (current[1] > point[1]) != (previous[1] > point[1]);
        if crosses {
            let z = previous[0]
                + (point[1] - previous[1]) * (current[0] - previous[0])
                    / (current[1] - previous[1]);
            if point[0] < z {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

/// Distancia de `point` a la poligonal cerrada `outline`.
fn distance_to_closed_line(outline: &[[f64; 2]], point: [f64; 2]) -> f64 {
    let mut previous = outline[outline.len() - 1];
    let mut best = f64::INFINITY;
    for &current in outline {
        best = best.min(distance_to_segment(previous, current, point));
        previous = current;
    }
    best
}

fn distance_to_segment(a: [f64; 2], b: [f64; 2], point: [f64; 2]) -> f64 {
    let (dz, dr) = (b[0] - a[0], b[1] - a[1]);
    let squared = dz * dz + dr * dr;
    let t = if squared > 0.0 {
        (((point[0] - a[0]) * dz + (point[1] - a[1]) * dr) / squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (point[0] - a[0] - t * dz).hypot(point[1] - a[1] - t * dr)
}

/// La tangente unitaria de una poligonal donde pasa `at`, en el sentido en que
/// se recorre.
fn wall_tangent(line: &[[f64; 2]], at: [f64; 2]) -> Option<[f64; 2]> {
    let (index, _) = line
        .windows(2)
        .enumerate()
        .map(|(index, pair)| (index, distance_to_segment(pair[0], pair[1], at)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    let (dz, dr) = (
        line[index + 1][0] - line[index][0],
        line[index + 1][1] - line[index][1],
    );
    let length = dz.hypot(dr);
    (length > 0.0).then(|| [dz / length, dr / length])
}

/// El borde de ataque que sigue el canal, de la tapa (primer punto) al cubo:
/// cada línea de corriente, a una fracción `phi` del cubo (0) a la tapa (1) de
/// la línea que une los puntos de las dos paredes a igual fracción de su
/// longitud, corta el borde donde esa fracción de pared va linealmente de la del
/// cubo (`hub`) a la de la tapa (`shroud`). Es la forma «canal» de la edición
/// del borde de las bombas de turbopump: con las dos fracciones iguales es la
/// recta que une los dos puntos y, si no, se curva con el canal. Con `count`
/// puntos.
fn channel_edge(
    front: &[[f64; 2]],
    rear: &[[f64; 2]],
    shroud: f64,
    hub: f64,
    count: usize,
) -> Vec<[f64; 2]> {
    let (front_lengths, rear_lengths) = (cumulative(front), cumulative(rear));
    let (front_total, rear_total) = (
        front_lengths[front_lengths.len() - 1],
        rear_lengths[rear_lengths.len() - 1],
    );
    (0..count)
        .map(|k| {
            // De la tapa (phi = 1) al cubo (phi = 0).
            let phi = 1.0 - k as f64 / (count - 1) as f64;
            let along = hub + phi * (shroud - hub);
            let on_hub = at_length(rear, &rear_lengths, along * rear_total);
            let on_shroud = at_length(front, &front_lengths, along * front_total);
            [
                on_hub[0] + phi * (on_shroud[0] - on_hub[0]),
                on_hub[1] + phi * (on_shroud[1] - on_hub[1]),
            ]
        })
        .collect()
}

/// Los seis valores de la Bézier de 5 puntos de `from` a `to` (P0 y P4) que más
/// se acerca por mínimos cuadrados a los puntos `samples` (con la longitud de
/// cuerda como parámetro), en el marco de la cuerda. Sin cuerda, o si el
/// sistema es singular, los brazos de un cuarto de la cuerda sobre ella.
fn fit_edge(from: [f64; 2], to: [f64; 2], samples: &[[f64; 2]]) -> EdgeFit {
    let chord = (to[0] - from[0]).hypot(to[1] - from[1]);
    let straight = EdgeFit {
        p2_along: 0.5,
        p2_offset: 0.0,
        start_angle: 0.0,
        start_length: (chord / 4.0).max(MIN_ARM),
        end_angle: 0.0,
        end_length: (chord / 4.0).max(MIN_ARM),
    };
    let Some((u, n)) = edge_frame(from, to) else {
        return straight;
    };
    let lengths = cumulative(samples);
    let total = lengths[lengths.len() - 1];
    if total <= 0.0 {
        return straight;
    }
    // Ecuaciones normales de P1, P2 y P3 con P0 y P4 fijos.
    let mut a = [[0.0; 4]; 3];
    let mut rhs = [[0.0; 2]; 3];
    for (point, length) in samples.iter().zip(&lengths) {
        let t = length / total;
        let s = 1.0 - t;
        let basis = [
            s.powi(4),
            4.0 * s.powi(3) * t,
            6.0 * s * s * t * t,
            4.0 * s * t.powi(3),
            t.powi(4),
        ];
        let residual = [
            point[0] - basis[0] * from[0] - basis[4] * to[0],
            point[1] - basis[0] * from[1] - basis[4] * to[1],
        ];
        for j in 0..3 {
            for k in 0..3 {
                a[j][k] += basis[j + 1] * basis[k + 1];
            }
            rhs[j][0] += basis[j + 1] * residual[0];
            rhs[j][1] += basis[j + 1] * residual[1];
        }
    }
    let Some(controls) = solve_normal_equations(a, rhs) else {
        return straight;
    };
    let along =
        |p: [f64; 2], origin: [f64; 2]| (p[0] - origin[0]) * u[0] + (p[1] - origin[1]) * u[1];
    let across =
        |p: [f64; 2], origin: [f64; 2]| (p[0] - origin[0]) * n[0] + (p[1] - origin[1]) * n[1];
    let (p1, p2, p3) = (controls[0], controls[1], controls[2]);
    let arm = |p: [f64; 2], origin: [f64; 2], direction: f64| {
        let (x, y) = (direction * along(p, origin), across(p, origin));
        (
            y.atan2(x).to_degrees().clamp(ANGLE_RANGE.0, ANGLE_RANGE.1),
            x.hypot(y).max(MIN_ARM),
        )
    };
    let (start_angle, start_length) = arm(p1, from, 1.0);
    let (end_angle, end_length) = arm(p3, to, -1.0);
    EdgeFit {
        p2_along: (along(p2, from) / chord).clamp(0.0, 1.0),
        p2_offset: across(p2, from),
        start_angle,
        start_length,
        end_angle,
        end_length,
    }
}

/// Resuelve A·x = b para tres incógnitas con dos columnas de segundo miembro
/// (z y r), por eliminación gaussiana con pivote. `None` si es singular.
fn solve_normal_equations(a: [[f64; 4]; 3], rhs: [[f64; 2]; 3]) -> Option<[[f64; 2]; 3]> {
    let mut m = [[0.0; 5]; 3];
    for j in 0..3 {
        m[j][..3].copy_from_slice(&a[j][..3]);
        m[j][3] = rhs[j][0];
        m[j][4] = rhs[j][1];
    }
    for col in 0..3 {
        let pivot = (col..3).max_by(|&x, &y| m[x][col].abs().total_cmp(&m[y][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        for row in 0..3 {
            if row != col {
                let factor = m[row][col] / m[col][col];
                let pivot_row = m[col];
                for (value, base) in m[row].iter_mut().zip(pivot_row).skip(col) {
                    *value -= factor * base;
                }
            }
        }
    }
    Some([
        [m[0][3] / m[0][0], m[0][4] / m[0][0]],
        [m[1][3] / m[1][1], m[1][4] / m[1][1]],
        [m[2][3] / m[2][2], m[2][4] / m[2][2]],
    ])
}

/// El ángulo del brazo del borde de ataque en el extremo `end` de la pared
/// `wall_line`, con la cuerda hacia `other` y en grados con una décima, con que
/// sale perpendicular a la pared, hacia el canal. `sign` es 1 si el extremo es
/// el de la tapa (inicio del borde) y −1 si es el del cubo. `None` sin cuerda o
/// sin tangente.
fn perpendicular_arm_angle(
    wall_line: &[[f64; 2]],
    end: [f64; 2],
    other: [f64; 2],
    sign: f64,
) -> Option<f64> {
    let (u, n) = if sign > 0.0 {
        edge_frame(end, other)?
    } else {
        edge_frame(other, end)?
    };
    let tangent = wall_tangent(wall_line, end)?;
    // De las dos perpendiculares, la que va hacia el otro extremo del borde.
    let mut direction = [-tangent[1], tangent[0]];
    if sign * (direction[0] * u[0] + direction[1] * u[1]) < 0.0 {
        direction = [-direction[0], -direction[1]];
    }
    let along = direction[0] * u[0] + direction[1] * u[1];
    let across = direction[0] * n[0] + direction[1] * n[1];
    let angle = across
        .atan2(sign * along)
        .to_degrees()
        .clamp(ANGLE_RANGE.0, ANGLE_RANGE.1);
    Some((angle * 10.0).round() / 10.0)
}

/// La mezcla de dos valores de un mismo punto del borde: `current` con `0`,
/// `wanted` con `1`. Otro tipo de movimiento no se mezcla.
fn blend(current: Moved, wanted: Moved, s: f64) -> Moved {
    let mix = |a: f64, b: f64| a + (b - a) * s;
    match (current, wanted) {
        (
            Moved::Arm {
                angle: a0,
                length: l0,
            },
            Moved::Arm {
                angle: a1,
                length: l1,
            },
        ) => Moved::Arm {
            angle: mix(a0, a1),
            length: mix(l0, l1),
        },
        (
            Moved::EdgePoint {
                along: a0,
                offset: o0,
            },
            Moved::EdgePoint {
                along: a1,
                offset: o1,
            },
        ) => Moved::EdgePoint {
            along: mix(a0, a1),
            offset: mix(o0, o1),
        },
        _ => wanted,
    }
}

/// Con la rejilla de las cajas: décimas de grado, centésimas de mm y milésimas
/// de cuerda.
fn rounded_move(moved: Moved) -> Moved {
    let round = |value: f64, scale: f64| (value * scale).round() / scale;
    match moved {
        Moved::Arm { angle, length } => Moved::Arm {
            angle: round(angle, 10.0),
            length: round(length, 100.0),
        },
        Moved::EdgePoint { along, offset } => Moved::EdgePoint {
            along: round(along, 1000.0),
            offset: round(offset, 100.0),
        },
        other => other,
    }
}

/// Los ejes del marco de la cuerda de `from` a `to`: el unitario de la cuerda
/// y la normal, (Δr, −Δz)/|cuerda|, que en un borde radial apunta a −z (aguas
/// arriba). `None` si los extremos coinciden.
fn edge_frame(from: [f64; 2], to: [f64; 2]) -> Option<([f64; 2], [f64; 2])> {
    let (dz, dr) = (to[0] - from[0], to[1] - from[1]);
    let chord = dz.hypot(dr);
    (chord >= RANGE_TOLERANCE).then(|| ([dz / chord, dr / chord], [dr / chord, -dz / chord]))
}

/// Los 5 puntos del borde de ataque con lo editado, validado como las tapas.
fn edge_shape(
    from: [f64; 2],
    to: [f64; 2],
    edit: &EdgeEdit,
    auto: &EdgeFit,
) -> Result<EdgeShape, DesignError> {
    let chord = (to[0] - from[0]).hypot(to[1] - from[1]);
    let number = |value: Option<f64>, auto: f64, what: &str, range: (f64, f64)| {
        let value = value.unwrap_or(auto);
        if value.is_finite() && (range.0..=range.1).contains(&value) {
            Ok(value)
        } else {
            Err(DesignError::Input(format!(
                "meridiano: {what} del borde de ataque vale {value} y tiene que estar entre {} y {}",
                range.0, range.1
            )))
        }
    };
    let p2_along = number(
        edit.p2_along,
        auto.p2_along,
        "la posición de P2",
        (0.0, 1.0),
    )?;
    let p2_offset = number(
        edit.p2_offset,
        auto.p2_offset,
        "el desvío de P2 (mm)",
        (-1.0e6, 1.0e6),
    )?;
    let start_angle = number(
        edit.start_angle,
        auto.start_angle,
        "el ángulo de inicio (grados)",
        ANGLE_RANGE,
    )?;
    let end_angle = number(
        edit.end_angle,
        auto.end_angle,
        "el ángulo final (grados)",
        ANGLE_RANGE,
    )?;
    let positive = |value: Option<f64>, auto: f64, what: &str| {
        let value = value.unwrap_or(auto);
        if value.is_finite() && value > 0.0 || (chord < RANGE_TOLERANCE && value == 0.0) {
            Ok(value)
        } else {
            Err(DesignError::Input(format!(
                "meridiano: {what} del borde de ataque vale {value} y tiene que ser mayor que 0 mm"
            )))
        }
    };
    let start_length = positive(
        edit.start_length,
        auto.start_length,
        "la longitud de inicio",
    )?;
    let end_length = positive(edit.end_length, auto.end_length, "la longitud final")?;
    let points = match edge_frame(from, to) {
        None => [from; 5],
        Some((u, n)) => {
            let arm = |origin: [f64; 2], angle: f64, length: f64, direction: f64| {
                let (sin, cos) = angle.to_radians().sin_cos();
                [
                    origin[0] + length * (direction * cos * u[0] + sin * n[0]),
                    origin[1] + length * (direction * cos * u[1] + sin * n[1]),
                ]
            };
            [
                from,
                arm(from, start_angle, start_length, 1.0),
                [
                    from[0] + p2_along * (to[0] - from[0]) + p2_offset * n[0],
                    from[1] + p2_along * (to[1] - from[1]) + p2_offset * n[1],
                ],
                arm(to, end_angle, end_length, -1.0),
                to,
            ]
        }
    };
    Ok(EdgeShape {
        points,
        chord,
        p2_along,
        p2_offset,
        start_angle,
        start_length,
        end_angle,
        end_length,
        auto: *auto,
    })
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
    /// Área de paso, 2π·r·b. En la primera estación de un borde de ataque
    /// curvo es el área de entrada sobre la curva, y la anchura sigue siendo la
    /// de la cuerda.
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

/// Momento estático S = ∫ r ds de la polilínea `line` (puntos `[z, r]`) desde
/// la fracción `from_fraction` de su longitud hasta el final, en mm²: la suma
/// de ½(r_i + r_{i+1})·|Δs|, con el primer tramo cortado por interpolación.
fn static_moment(line: &[[f64; 2]], from_fraction: f64) -> f64 {
    let lengths = cumulative(line);
    let start = from_fraction * lengths[lengths.len() - 1];
    let mut points = vec![at_length(line, &lengths, start)];
    for (point, length) in line.iter().zip(&lengths) {
        if *length > start {
            points.push(*point);
        }
    }
    points
        .windows(2)
        .map(|pair| {
            0.5 * (pair[0][1] + pair[1][1])
                * (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1])
        })
        .sum()
}

/// La fracción del borde de ataque sobre el cubo. Escrita, manda. Si no, es la
/// que hace el momento estático del cubo igual al de la tapa desde su fracción
/// (criterio de igual transmisión de energía en cubo y tapa; momento estático
/// de Pfleiderer, ver notas de estudio). S del cubo decrece con la fracción:
/// bisección en [`EDGE_FRACTION_RANGE`]. En un límite, se queda ahí y lo anota.
fn edge_hub_fraction(
    front: &[[f64; 2]],
    rear: &[[f64; 2]],
    edge_shroud: f64,
    written: Option<f64>,
) -> (f64, EdgeAuto) {
    let s_shroud = static_moment(front, edge_shroud);
    if let Some(fraction) = written {
        let auto = EdgeAuto {
            hub_auto: false,
            s_shroud,
            s_hub: static_moment(rear, fraction),
            clamp: None,
        };
        return (fraction, auto);
    }
    let (low, high) = EDGE_FRACTION_RANGE;
    let (fraction, clamp) = if static_moment(rear, low) <= s_shroud {
        (low, Some(EdgeClamp::AtInlet))
    } else if static_moment(rear, high) > s_shroud {
        (high, Some(EdgeClamp::AtLimit))
    } else {
        let (mut lo, mut hi) = (low, high);
        while hi - lo > 1e-9 {
            let mid = 0.5 * (lo + hi);
            if static_moment(rear, mid) > s_shroud {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (0.5 * (lo + hi), None)
    };
    let auto = EdgeAuto {
        hub_auto: true,
        s_shroud,
        s_hub: static_moment(rear, fraction),
        clamp,
    };
    (fraction, auto)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_moment_is_the_integral_of_r_ds_from_the_fraction() {
        // Recta horizontal a r = 10 y 10 mm de largo: S = 100; desde la mitad, 50.
        let line = [[0.0, 10.0], [4.0, 10.0], [10.0, 10.0]];
        assert!((static_moment(&line, 0.0) - 100.0).abs() < 1e-12);
        assert!((static_moment(&line, 0.5) - 50.0).abs() < 1e-12);
        // Recta inclinada de r = 0 a r = 10 en 10 mm: S = ½·10·10.
        let ramp = [[0.0, 0.0], [10.0, 10.0]];
        assert!((static_moment(&ramp, 0.0) - 0.5 * 10.0 * 10.0 * 2f64.sqrt()).abs() < 1e-9);
    }

    #[test]
    fn edge_hub_fraction_pins_to_the_inlet_when_the_hub_moment_is_already_smaller() {
        let front = [[0.0, 10.0], [10.0, 10.0]];
        let rear = [[0.0, 1.0], [10.0, 1.0]];
        let (fraction, auto) = edge_hub_fraction(&front, &rear, 0.0, None);
        assert_eq!(fraction, 0.0);
        assert_eq!(auto.clamp, Some(EdgeClamp::AtInlet));
        assert!(auto.hub_auto);
    }

    #[test]
    fn edge_hub_fraction_bisects_to_equal_moments() {
        let front = [[0.0, 10.0], [10.0, 10.0]];
        let rear = [[0.0, 20.0], [20.0, 20.0]];
        // S tapa = 100; S cubo(f) = 20·20·(1 − f) = 100 → f = 0,75.
        let (fraction, auto) = edge_hub_fraction(&front, &rear, 0.0, None);
        assert!((fraction - 0.75).abs() < 1e-6, "{fraction}");
        assert_eq!(auto.clamp, None);
        assert!((auto.s_hub - auto.s_shroud).abs() < 1e-4);
    }
}
