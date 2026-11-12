//! Las dos caras de cada línea de corriente de un álabe con espesor.
//!
//! Módulo puro: entra la línea media ([`BladeCamber`]), las líneas de corriente
//! del meridiano, las curvas de espesor y el número de álabes; salen, por línea
//! de corriente, la cara de presión, la línea media remuestreada y la de
//! succión. No conoce la interfaz ni el visor.
//!
//! # Construcción
//!
//! Todo se hace en el plano conforme (m′, θ) de cada línea, donde
//! ds² = r²·(dm′² + dθ²) (Srinivasan, *Rotodynamic Pumps*, 2.ª ed., ec. 4.61)
//! y los ángulos son los verdaderos. En cada nodo de la línea media se
//! desplaza ±(e/2)/r a lo largo de la normal, y el punto desplazado vuelve a
//! (z, r) por la misma línea de corriente (de m′ a m, y de m a z y r), de modo
//! que las caras del cubo y de la tapa caen sobre el cubo y la tapa. De hmflow.
//!
//! - **Camino.** La línea de corriente sirve antes del borde de ataque y, si
//!   ahí se acaba o no existe, se prolonga en recta su primer tramo. Después de
//!   la salida se prolonga en recta el último. Ambas prolongaciones valen
//!   [`SOLID_EXTENSION_STEPS`] pasos y llegan a Δm = e(0) y e(1).
//! - **Línea media prolongada.** Más allá de x = 1, θ = θ_J + (m′ − m′_J)/tan β_J:
//!   β constante, el del último nodo.
//! - **Presión y succión.** El giro es antihorario visto desde la aspiración;
//!   en las coordenadas de [`BladeCamber`] (θ crece de la entrada a la salida)
//!   va hacia −θ: la cara de presión es la del lado −θ.
//! - **Cantos.** El de entrada es romo (corte perpendicular a la línea media)
//!   y el de salida queda recortado en la salida del rodete, con las dos caras
//!   cortadas a m = m_salida. Los dos son caras propias: `pressure` y `suction`
//!   acaban en su arista y el canto se lee con [`SolidLine::leading_edge`] y
//!   [`SolidLine::trailing_edge`].
//! - **Nariz** (con [`LeadingKind::Ellipse`] o [`LeadingKind::Wedge`]). El
//!   espesor de cada cara en la nariz es el de
//!   [`crate::blade_edges::NoseProfile::half_thicknesses`], con `s` la longitud
//!   verdadera de la línea media desde la punta (el arco del plano conforme por
//!   el r de cada tramo: la longitud de la superficie de corriente, que es la
//!   que acota Lp) y e = e(x) del pie. Las filas de la nariz (`nose`) van de
//!   la punta a la unión, en [`NOSE_STEPS`] pasos a ángulo de elipse uniforme
//!   (más densas hacia la punta, de hmflow); `pressure`, `middle` y `suction`
//!   empiezan en la unión, que es el canto romo de una línea media que arranca
//!   ahí: el mismo código de la etapa 1. Con romo no hay nariz y nada cambia.
//!
//! El desplazamiento es exacto en ángulo y aproximado en distancia: el error es
//! del orden de (e/r)², mayor en el cubo.

use thiserror::Error;

use crate::blade_camber::{BladeCamber, CAMBER_MIN_RADIUS, CamberPoint, camber_for_design};
use crate::blade_edges::{
    BladeEdges, LeadingDims, LeadingKind, NOSE_LONG_FRACTION, NOSE_MAX_FRACTION, NoseProfile,
};
use crate::blade_setup::BladeSetup;
use crate::blade_thickness::{ThicknessCurves, thickness_curves_for_design};
use crate::geometry::ImpellerGeometry;
use crate::meridian::{MeridionalModel, cumulative};
use crate::streamlines::{Streamline, streamlines};
use crate::{DesignError, DesignResult};

/// Pasos con que se prolonga el camino aguas arriba del borde de ataque y
/// aguas abajo de la salida. De hmflow.
pub const SOLID_EXTENSION_STEPS: usize = 8;

/// Pasos por lado de la nariz, de la punta a la unión: 16, a ángulo de elipse
/// uniforme. De hmflow: con ellos la cuerda no se aparta de la elipse más de
/// 0,05 mm (la tolerancia fina de la escena) en una nariz de hasta unos 40 mm.
pub const NOSE_STEPS: usize = 16;

/// Lo que puede diferir el radio de salida de una línea del de la línea 0 para
/// que la salida se considere radial, m.
pub const RADIAL_OUTLET_TOLERANCE: f64 = 1.0e-6;

/// Un punto de una cara, en m y rad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidPoint {
    pub z: f64,
    pub r: f64,
    pub theta: f64,
    /// m′ = ∫dm/r, con el origen en el nodo 0 de la línea media (como
    /// [`CamberPoint::m_conformal`]).
    pub m_conformal: f64,
}

/// Las dos caras y la línea media de una línea de corriente.
#[derive(Debug, Clone, PartialEq)]
pub struct SolidLine {
    pub psi: f64,
    /// J + 1 puntos cada una, de la unión de la nariz a la salida (del borde
    /// de ataque si es romo).
    pub pressure: Vec<SolidPoint>,
    pub middle: Vec<SolidPoint>,
    pub suction: Vec<SolidPoint>,
    /// Filas [presión, medio, succión] de la punta (s = 0) a la unión
    /// (s = Lp): [`NOSE_STEPS`] + 1, la última igual a `pressure[0]`,
    /// `middle[0]` y `suction[0]`. Vacía con romo.
    pub nose: Vec<[SolidPoint; 3]>,
    /// Espesor real en cada nodo de la línea media (J + 1), m: la distancia
    /// entre las dos caras medida sobre la normal a la superficie media.
    pub true_thickness: Vec<f64>,
}

impl SolidLine {
    /// La punta de la entrada: el medio de la primera fila de la nariz; con
    /// romo, `middle[0]`.
    #[must_use]
    pub fn leading_tip(&self) -> SolidPoint {
        self.nose.first().map_or(self.middle[0], |row| row[1])
    }

    /// El canto de entrada sobre esta línea: presión, medio y succión de la
    /// estación 0.
    #[must_use]
    pub fn leading_edge(&self) -> [SolidPoint; 3] {
        [self.pressure[0], self.middle[0], self.suction[0]]
    }

    /// El canto de salida: los de la última estación.
    #[must_use]
    pub fn trailing_edge(&self) -> [SolidPoint; 3] {
        let last = self.pressure.len() - 1;
        [self.pressure[last], self.middle[last], self.suction[last]]
    }
}

/// Las caras de todas las líneas de corriente, del cubo a la tapa.
#[derive(Debug, Clone, PartialEq)]
pub struct BladeSolid {
    pub lines: Vec<SolidLine>,
    pub readings: SolidReadings,
    pub notices: Vec<SolidNotice>,
}

/// Un radio en m como «45,1»: mm con un decimal y coma.
fn mm(radius: &f64) -> String {
    format!("{:.1}", radius * 1000.0).replace('.', ",")
}

/// Por qué no sale el álabe con espesor. Ninguno se arregla en silencio.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum BladeSolidError {
    #[error("no hay línea media 3D y el álabe con espesor no se construye: {0}")]
    NoCamber(String),

    #[error("las líneas de corriente no son las de la línea media")]
    LineMismatch,

    /// `radius` en m; `at` es (m′, θ) del nodo.
    #[error(
        "línea de corriente {line} (0 es el cubo): las caras del álabe se cruzan en r = {} mm: el radio de curvatura de la línea media baja de medio espesor",
        mm(.radius)
    )]
    FacesCross {
        line: usize,
        radius: f64,
        at: [f64; 2],
    },

    #[error(
        "línea de corriente {line} (0 es el cubo): los álabes vecinos se tocan en r = {} mm",
        mm(.radius)
    )]
    NeighboursTouch {
        line: usize,
        radius: f64,
        at: [f64; 2],
    },

    /// La nariz no cabe en la línea ni con la cota acotada. `radius`, `lp` y
    /// `limit` en m.
    #[error(
        "línea de corriente {line} (0 es el cubo): la nariz no cabe en la línea en r = {} mm: Lp = {} mm y el máximo es {} mm",
        mm(.radius),
        mm(.lp),
        mm(.limit)
    )]
    NoseOutOfRange {
        line: usize,
        radius: f64,
        lp: f64,
        limit: f64,
    },

    #[error(
        "línea de corriente {line} (0 es el cubo): la salida del rodete no es radial (r = {} mm): el recorte del álabe en una salida inclinada no está resuelto",
        mm(.radius)
    )]
    InclinedOutlet { line: usize, radius: f64 },
}

/// El camino de una línea de corriente: su polilínea (con las dos
/// prolongaciones en recta) y la tabla m ↔ m′ de la línea media y sus dos
/// prolongaciones. `m` es la longitud desde el nodo 0 de la línea media,
/// negativa aguas arriba de él.
struct Path {
    /// `[z, r]`, m.
    polyline: Vec<[f64; 2]>,
    lengths: Vec<f64>,
    /// Longitud de la línea de corriente hasta el nodo 0, m.
    m_start: f64,
    first_direction: [f64; 2],
    last_direction: [f64; 2],
    /// Estrictamente crecientes.
    table_m: Vec<f64>,
    table_conformal: Vec<f64>,
}

impl Path {
    fn new(line: &Streamline) -> Option<Self> {
        let polyline: Vec<[f64; 2]> = line
            .points
            .iter()
            .map(|point| [point[0] / 1000.0, point[1] / 1000.0])
            .collect();
        if polyline.len() < 2 {
            return None;
        }
        let lengths = cumulative(&polyline);
        let direction = |pair: &[[f64; 2]]| {
            let (dz, dr) = (pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]);
            let length = dz.hypot(dr);
            (length > 0.0).then(|| [dz / length, dr / length])
        };
        let first_direction = polyline.windows(2).find_map(direction)?;
        let last_direction = polyline.windows(2).rev().find_map(direction)?;
        Some(Self {
            polyline,
            lengths,
            m_start: line.m_leading_edge / 1000.0,
            first_direction,
            last_direction,
            table_m: Vec::new(),
            table_conformal: Vec::new(),
        })
    }

    /// (z, r) a longitud `m` del nodo 0. Fuera de la línea de corriente, su
    /// primer o último tramo prolongado en recta; el radio no baja del mínimo.
    fn position(&self, m: f64) -> [f64; 2] {
        let s = self.m_start + m;
        let total = self.lengths[self.lengths.len() - 1];
        let raw = if s <= 0.0 {
            let first = self.polyline[0];
            [
                first[0] + self.first_direction[0] * s,
                first[1] + self.first_direction[1] * s,
            ]
        } else if s >= total {
            let last = self.polyline[self.polyline.len() - 1];
            [
                last[0] + self.last_direction[0] * (s - total),
                last[1] + self.last_direction[1] * (s - total),
            ]
        } else {
            let segment = (self.lengths.partition_point(|length| *length <= s) - 1)
                .min(self.polyline.len() - 2);
            let run = self.lengths[segment + 1] - self.lengths[segment];
            let t = if run > 0.0 {
                ((s - self.lengths[segment]) / run).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (a, b) = (self.polyline[segment], self.polyline[segment + 1]);
            [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
        };
        [raw[0], raw[1].max(CAMBER_MIN_RADIUS)]
    }

    /// La tabla m ↔ m′: los nodos de la línea media y las prolongaciones de
    /// `e_in` aguas arriba y `e_out` aguas abajo. m′ sigue por trapecios de
    /// dm/r.
    fn with_table(mut self, nodes: &[CamberPoint], e_in: f64, e_out: f64) -> Self {
        let first = &nodes[0];
        let last = &nodes[nodes.len() - 1];
        let steps = SOLID_EXTENSION_STEPS;
        let extend = |from: &CamberPoint, length: f64, sign: f64| -> Vec<(f64, f64)> {
            let mut out = Vec::with_capacity(steps);
            let (mut m_prev, mut mp_prev) = (from.m, from.m_conformal);
            let mut r_prev = self.position(m_prev)[1];
            for i in 1..=steps {
                let m = from.m + sign * length * i as f64 / steps as f64;
                let r = self.position(m)[1];
                let mp = mp_prev + sign * 0.5 * (1.0 / r_prev + 1.0 / r) * (m - m_prev).abs();
                out.push((m, mp));
                (m_prev, mp_prev, r_prev) = (m, mp, r);
            }
            out
        };
        let upstream = extend(first, e_in, -1.0);
        let downstream = extend(last, e_out, 1.0);

        let mut table: Vec<(f64, f64)> = upstream.into_iter().rev().collect();
        table.extend(nodes.iter().map(|point| (point.m, point.m_conformal)));
        table.extend(downstream);
        self.table_m = table.iter().map(|entry| entry.0).collect();
        self.table_conformal = table.iter().map(|entry| entry.1).collect();
        self
    }

    /// La longitud m a que corresponde `conformal`, por interpolación lineal
    /// en la tabla (fuera de ella, el extremo).
    fn m_of(&self, conformal: f64) -> f64 {
        let table = &self.table_conformal;
        let last = table.len() - 1;
        if conformal <= table[0] {
            return self.table_m[0];
        }
        if conformal >= table[last] {
            return self.table_m[last];
        }
        let segment = (table.partition_point(|value| *value <= conformal) - 1).min(last - 1);
        let run = table[segment + 1] - table[segment];
        let t = if run > 0.0 {
            (conformal - table[segment]) / run
        } else {
            0.0
        };
        self.table_m[segment] + t * (self.table_m[segment + 1] - self.table_m[segment])
    }

    /// (z, r) del punto con esa m′.
    fn at(&self, conformal: f64) -> [f64; 2] {
        self.position(self.m_of(conformal))
    }
}

/// Un nodo de la línea media, real o de la prolongación.
#[derive(Clone, Copy)]
struct Station {
    conformal: f64,
    theta: f64,
    /// Espesor en este nodo, m.
    e: f64,
    /// m.
    r: f64,
}

/// Un punto de cara en el plano conforme.
#[derive(Clone, Copy)]
struct Plane {
    conformal: f64,
    theta: f64,
}

/// Unitario de `to − from` en el plano conforme.
fn unit(from: Plane, to: Plane) -> [f64; 2] {
    let (dm, dt) = (to.conformal - from.conformal, to.theta - from.theta);
    let length = dm.hypot(dt);
    if length > 0.0 {
        [dm / length, dt / length]
    } else {
        [1.0, 0.0]
    }
}

/// Tangente unitaria (t_m′, t_θ) de cada nodo: diferencias centradas y,
/// laterales en los extremos.
fn tangents(planes: &[Plane]) -> Vec<[f64; 2]> {
    let last = planes.len() - 1;
    (0..=last)
        .map(|i| unit(planes[i.saturating_sub(1)], planes[(i + 1).min(last)]))
        .collect()
}

/// Iteraciones con que el espesor de un punto desplazado se evalúa en su
/// propia x. Cada pasada acorta el error a un tercio en el adelgazamiento de
/// «Gülich» (el más brusco); con 8 queda por debajo de 1 µm.
const FIXED_POINT_PASSES: usize = 8;

/// Bisecciones de la búsqueda del corte de la salida: 2⁻⁵⁰ del tramo.
const CUT_BISECTIONS: usize = 50;

/// La x de la línea media y su espesor como funciones de m′.
struct Along<'a> {
    /// (m′, x) de los nodos de la línea media.
    nodes: Vec<(f64, f64)>,
    thickness: Box<dyn Fn(f64) -> f64 + 'a>,
}

impl Along<'_> {
    /// El espesor en la x que corresponde a `conformal`: 0 antes del nodo 0 y
    /// 1 más allá del último.
    fn thickness(&self, conformal: f64) -> f64 {
        let last = self.nodes.len() - 1;
        let x = if conformal <= self.nodes[0].0 {
            0.0
        } else if conformal >= self.nodes[last].0 {
            1.0
        } else {
            let segment =
                (self.nodes.partition_point(|node| node.0 <= conformal) - 1).min(last - 1);
            let (a, b) = (self.nodes[segment], self.nodes[segment + 1]);
            a.1 + (conformal - a.0) / (b.0 - a.0) * (b.1 - a.1)
        };
        (self.thickness)(x)
    }
}

/// Cómo se desplaza un pie de la línea media.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// ±(e/2)/r con el espesor y el r del pie: el nodo 0, para que el canto de
    /// entrada quede perpendicular a la línea media.
    Foot,
    /// Con el espesor del pie y r a medio desplazamiento: la sección nominal
    /// del nodo, de donde se lee el espesor real.
    Section,
    /// Con el espesor de la x del punto desplazado y r a medio desplazamiento:
    /// las caras.
    Face,
}

/// El punto de una cara que sale del pie `foot` de la línea media, de tangente
/// `t`, espesor nominal `e` y radio `r`: ±(e/2)/r a lo largo de la normal
/// (t_θ, −t_m′), con + hacia la presión.
///
/// En las caras ([`Mode::Face`]), (a) el espesor es el de la x del punto
/// desplazado y no el de su pie, para que las dos caras lleguen al corte de la
/// salida con e(1) y el canto mida e(1)/sen β, el valor del bloqueo del 1D; y
/// (b) r se evalúa a medio desplazamiento, porque el desplazamiento solo se
/// compensa entre las dos caras si el r es el mismo, y en la salida salen de
/// nodos con r distinto. Donde el espesor es constante y r casi, no cambia
/// nada.
#[allow(clippy::too_many_arguments)]
fn offset(
    sign: f64,
    foot: Plane,
    t: [f64; 2],
    e: f64,
    r: f64,
    mode: Mode,
    path: &Path,
    along: &Along,
) -> Plane {
    let displaced = |e: f64, r: f64| {
        let half = sign * 0.5 * e / r;
        Plane {
            conformal: foot.conformal + half * t[1],
            theta: foot.theta - half * t[0],
        }
    };
    let mut point = displaced(e, r);
    if mode != Mode::Foot {
        for _ in 0..FIXED_POINT_PASSES {
            let middle = 0.5 * (foot.conformal + point.conformal);
            let e = if mode == Mode::Face {
                along.thickness(point.conformal)
            } else {
                e
            };
            point = displaced(e, path.at(middle)[1]);
        }
    }
    point
}

/// Una cara: la polilínea desplazada de la línea media, cortada en m′ = m′_J y
/// remuestreada en J + 1 puntos a pasos iguales de m′. `steps` es el índice de
/// la estación de la salida en las listas dadas y `first`, cómo se desplaza la
/// estación 0: [`Mode::Foot`] en el canto romo y [`Mode::Face`] en la unión de
/// la nariz.
#[allow(clippy::too_many_arguments)]
fn face(
    line: usize,
    path: &Path,
    stations: &[Station],
    planes: &[Plane],
    tangents: &[[f64; 2]],
    along: &Along,
    sign: f64,
    steps: usize,
    intervals: usize,
    first: Mode,
) -> Result<Vec<SolidPoint>, BladeSolidError> {
    let shifted: Vec<Plane> = stations
        .iter()
        .zip(planes)
        .zip(tangents)
        .enumerate()
        .map(|(index, ((station, plane), t))| {
            offset(
                sign,
                *plane,
                *t,
                station.e,
                station.r,
                if index == 0 { first } else { Mode::Face },
                path,
                along,
            )
        })
        .collect();

    let end = planes[steps].conformal;
    let cut = shifted
        .iter()
        .position(|point| point.conformal >= end)
        .unwrap_or(shifted.len() - 1);
    // Un tramo que avanza contra la línea media, o que no avanza en m′, deja
    // una cara que no se puede muestrear: las caras se cruzan.
    for i in 0..cut {
        let (dm, dt) = (
            shifted[i + 1].conformal - shifted[i].conformal,
            shifted[i + 1].theta - shifted[i].theta,
        );
        if dm * tangents[i][0] + dt * tangents[i][1] <= 0.0 || dm <= 0.0 {
            return Err(BladeSolidError::FacesCross {
                line,
                radius: stations[i].r,
                at: [planes[i].conformal, planes[i].theta],
            });
        }
    }

    // El corte en m′ = m′_J, exacto: el espesor tiene un codo en x = 1 (baja
    // con pendiente y luego es constante) y interpolar entre los dos puntos
    // desplazados que lo rodean engrosaría el canto. Se busca, entre los pies
    // de esos dos puntos, el que desplaza justo a m′_J.
    let cut_theta = {
        let low = cut.saturating_sub(1);
        let (a, b) = (planes[low], planes[cut]);
        let at = |u: f64| {
            let foot = Plane {
                conformal: a.conformal + u * (b.conformal - a.conformal),
                theta: a.theta + u * (b.theta - a.theta),
            };
            let t = [
                tangents[low][0] + u * (tangents[cut][0] - tangents[low][0]),
                tangents[low][1] + u * (tangents[cut][1] - tangents[low][1]),
            ];
            let norm = t[0].hypot(t[1]);
            let t = if norm > 0.0 {
                [t[0] / norm, t[1] / norm]
            } else {
                tangents[low]
            };
            let e = stations[low].e + u * (stations[cut].e - stations[low].e);
            offset(
                sign,
                foot,
                t,
                e,
                path.at(foot.conformal)[1],
                Mode::Face,
                path,
                along,
            )
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..CUT_BISECTIONS {
            let middle = 0.5 * (lo + hi);
            if at(middle).conformal < end {
                lo = middle;
            } else {
                hi = middle;
            }
        }
        at(0.5 * (lo + hi)).theta
    };

    let start = shifted[0].conformal;
    let mut segment = 0;
    let mut out = Vec::with_capacity(intervals + 1);
    for k in 0..=intervals {
        let target = if k == intervals {
            end
        } else {
            start + (end - start) * k as f64 / intervals as f64
        };
        while segment + 1 < cut && shifted[segment + 1].conformal < target {
            segment += 1;
        }
        let (a, b) = (shifted[segment], shifted[(segment + 1).min(cut)]);
        let run = b.conformal - a.conformal;
        let t = if run > 0.0 {
            ((target - a.conformal) / run).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let [z, r] = path.at(target);
        out.push(SolidPoint {
            z,
            r,
            theta: if k == intervals {
                cut_theta
            } else {
                a.theta + t * (b.theta - a.theta)
            },
            m_conformal: target,
        });
    }
    Ok(out)
}

/// La sección nominal de cada uno de los J + 1 nodos de la línea media, sin
/// recortar (±e(x)/2), de donde se lee el espesor real.
#[allow(clippy::too_many_arguments)]
fn nominal_sections(
    path: &Path,
    stations: &[Station],
    planes: &[Plane],
    tangents: &[[f64; 2]],
    along: &Along,
    sign: f64,
    steps: usize,
) -> Vec<SolidPoint> {
    (0..=steps)
        .map(|i| {
            let mode = if i == 0 { Mode::Foot } else { Mode::Section };
            let point = offset(
                sign,
                planes[i],
                tangents[i],
                stations[i].e,
                stations[i].r,
                mode,
                path,
                along,
            );
            let [z, r] = path.at(point.conformal);
            SolidPoint {
                z,
                r,
                theta: point.theta,
                m_conformal: point.conformal,
            }
        })
        .collect()
}

/// θ de una cara muestreada en la m′ dada (lineal; fuera, el extremo).
fn theta_at(face: &[SolidPoint], conformal: f64) -> f64 {
    let last = face.len() - 1;
    if conformal <= face[0].m_conformal {
        return face[0].theta;
    }
    if conformal >= face[last].m_conformal {
        return face[last].theta;
    }
    let segment = (face.partition_point(|point| point.m_conformal <= conformal) - 1).min(last - 1);
    let (a, b) = (face[segment], face[segment + 1]);
    let run = b.m_conformal - a.m_conformal;
    let t = if run > 0.0 {
        (conformal - a.m_conformal) / run
    } else {
        0.0
    };
    a.theta + t * (b.theta - a.theta)
}

/// El punto medio de una pareja de puntos de las dos caras: m′ y θ medios, y
/// (z, r) de la línea de corriente en esa m′.
fn mid_point(pressure: &SolidPoint, suction: &SolidPoint, path: &Path) -> SolidPoint {
    let conformal = 0.5 * (pressure.m_conformal + suction.m_conformal);
    let [z, r] = path.at(conformal);
    SolidPoint {
        z,
        r,
        theta: 0.5 * (pressure.theta + suction.theta),
        m_conformal: conformal,
    }
}

/// El punto de una cara en el plano conforme, vuelto a (z, r).
fn solid_point(plane: Plane, path: &Path) -> SolidPoint {
    let [z, r] = path.at(plane.conformal);
    SolidPoint {
        z,
        r,
        theta: plane.theta,
        m_conformal: plane.conformal,
    }
}

/// Un pie de la línea media a una distancia `s` de la punta: su punto del plano
/// conforme, su tangente unitaria y su r.
struct Foot {
    plane: Plane,
    tangent: [f64; 2],
    r: f64,
}

/// El arco verdadero de la línea media en cada una de las estaciones 0..=J, m:
/// la longitud de la superficie de corriente, r·|d(m′, θ)| por tramo con el r
/// de la m′ media del tramo (Srinivasan, ec. 4.61). La nariz y su tope de
/// 0,3·L se miden con él.
fn arcs(path: &Path, planes: &[Plane], steps: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(steps + 1);
    out.push(0.0);
    for i in 1..=steps {
        let (a, b) = (planes[i - 1], planes[i]);
        let r = path.at(0.5 * (a.conformal + b.conformal))[1];
        let step = r * (b.conformal - a.conformal).hypot(b.theta - a.theta);
        out.push(out[i - 1] + step);
    }
    out
}

/// El pie a la distancia `s` de la punta, por interpolación lineal de la
/// línea media entre estaciones y de su tangente (renormalizada).
fn foot_at(s: f64, arc: &[f64], planes: &[Plane], tangents: &[[f64; 2]], path: &Path) -> Foot {
    let last = arc.len() - 1;
    let segment = arc
        .partition_point(|value| *value <= s)
        .saturating_sub(1)
        .min(last - 1);
    let run = arc[segment + 1] - arc[segment];
    let u = if run > 0.0 {
        ((s - arc[segment]) / run).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (a, b) = (planes[segment], planes[segment + 1]);
    let plane = Plane {
        conformal: a.conformal + u * (b.conformal - a.conformal),
        theta: a.theta + u * (b.theta - a.theta),
    };
    let t = [
        tangents[segment][0] + u * (tangents[segment + 1][0] - tangents[segment][0]),
        tangents[segment][1] + u * (tangents[segment + 1][1] - tangents[segment][1]),
    ];
    let norm = t[0].hypot(t[1]);
    let tangent = if norm > 0.0 {
        [t[0] / norm, t[1] / norm]
    } else {
        tangents[segment]
    };
    Foot {
        plane,
        tangent,
        r: path.at(plane.conformal)[1],
    }
}

/// El punto de una cara que sale del pie `foot` de la línea media, a la
/// distancia `s` de la punta: ±h/r a lo largo de su normal (t_θ, −t_m′), con +
/// hacia la presión, con h el semiespesor de esa cara en `profile` y r a medio
/// desplazamiento. Como en [`Mode::Face`], e se toma de la x del punto
/// desplazado y no de la del pie: así la nariz y las caras, que siguen con
/// ese mismo criterio, se unen sin escalón aunque e(x) tenga pendiente.
fn nose_offset(
    sign: f64,
    foot: &Foot,
    s: f64,
    e_foot: f64,
    profile: &NoseProfile,
    along: &Along,
    path: &Path,
) -> Plane {
    let half = |e: f64| {
        let (pressure, suction) = profile.half_thicknesses(s, e);
        if sign > 0.0 { pressure } else { suction }
    };
    let displaced = |half: f64, r: f64| {
        let shift = sign * half / r;
        Plane {
            conformal: foot.plane.conformal + shift * foot.tangent[1],
            theta: foot.plane.theta - shift * foot.tangent[0],
        }
    };
    let mut point = displaced(half(e_foot), foot.r);
    for _ in 0..FIXED_POINT_PASSES {
        let middle = 0.5 * (foot.plane.conformal + point.conformal);
        let e = along.thickness(point.conformal);
        point = displaced(half(e), path.at(middle)[1]);
    }
    point
}

/// El tramo de nariz de una línea: los pies de las filas (de la punta a la
/// unión, esta última no incluida), la unión y las estaciones de las caras
/// que arrancan en ella.
struct Nose {
    profile: NoseProfile,
    /// Un pie, su e y su distancia s a la punta por fila, de s = 0 a s_{N−1}.
    rows: Vec<(Foot, f64, f64)>,
    /// Las listas de las caras: la unión y las estaciones que le siguen.
    stations: Vec<Station>,
    planes: Vec<Plane>,
    tangents: Vec<[f64; 2]>,
    /// El índice de la estación de la salida en esas listas.
    end: usize,
    notices: Vec<SolidNotice>,
}

/// Las filas de la nariz de la línea `index`: acota las cotas a sus límites
/// (y lo dice), localiza la unión a s = Lp y deja las listas de las caras
/// que parten de ella. `Err(NoseOutOfRange)` solo si ni acotada cabe.
#[allow(clippy::too_many_arguments)]
fn plan_nose(
    index: usize,
    dims: LeadingDims,
    kind: LeadingKind,
    path: &Path,
    nodes: &[CamberPoint],
    stations: &[Station],
    planes: &[Plane],
    tangents: &[[f64; 2]],
    along: &Along,
    e0: f64,
) -> Result<Nose, BladeSolidError> {
    let steps = nodes.len() - 1;
    let arc = arcs(path, planes, steps);
    let length = arc[steps];
    let out_of_range = |lp: f64| BladeSolidError::NoseOutOfRange {
        line: index,
        radius: nodes[0].r,
        lp,
        limit: NOSE_MAX_FRACTION * length,
    };
    let clamped = dims
        .clamped(e0, length)
        .ok_or_else(|| out_of_range(dims.lp))?;
    if clamped.lp > NOSE_MAX_FRACTION * length {
        return Err(out_of_range(clamped.lp));
    }
    let profile = NoseProfile {
        kind,
        dims: clamped,
    };

    let at = [nodes[0].m_conformal, nodes[0].theta];
    let line = |text: String| SolidNotice {
        line: index,
        at,
        message: format!("línea de corriente {index} (0 es el cubo): {text}"),
    };
    let mut notices = Vec::new();
    if clamped.lp != dims.lp {
        notices.push(line(format!(
            "la longitud de la nariz Lp = {} mm se acotó a {} mm (entre e/2 = {} mm y 0,3·L = {} mm)",
            comma(dims.lp * 1000.0, 2),
            comma(clamped.lp * 1000.0, 2),
            comma(e0 / 2.0 * 1000.0, 2),
            comma(NOSE_MAX_FRACTION * length * 1000.0, 2),
        )));
    }
    if kind == LeadingKind::Wedge && clamped.tip != dims.tip {
        notices.push(line(format!(
            "la punta del afilado de {} mm se acotó a {} mm (entre 0 y e = {} mm)",
            comma(dims.tip * 1000.0, 2),
            comma(clamped.tip * 1000.0, 2),
            comma(e0 * 1000.0, 2),
        )));
    }
    if kind == LeadingKind::Wedge && clamped.asymmetry != dims.asymmetry {
        notices.push(line(format!(
            "la asimetría del afilado {} se acotó a {} (entre 0 y 1)",
            comma(dims.asymmetry, 2),
            comma(clamped.asymmetry, 2),
        )));
    }
    if clamped.lp > NOSE_LONG_FRACTION * length {
        notices.push(line(format!(
            "la nariz mide {} mm, más de 0,2·L = {} mm: un perfil largo y fino es malo de fundir",
            comma(clamped.lp * 1000.0, 2),
            comma(NOSE_LONG_FRACTION * length * 1000.0, 2),
        )));
    }

    // Las filas, a ángulo de elipse uniforme: s = Lp·(1 − cos φ).
    let rows = (0..NOSE_STEPS)
        .map(|k| {
            let u = if k == 0 {
                0.0
            } else {
                1.0 - (std::f64::consts::FRAC_PI_2 * k as f64 / NOSE_STEPS as f64).cos()
            };
            let s = u * clamped.lp;
            let foot = foot_at(s, &arc, planes, tangents, path);
            let e = along.thickness(foot.plane.conformal);
            (foot, e, s)
        })
        .collect();

    // La unión y las estaciones que le siguen. Una estación a menos de
    // e(0)/1000 de la unión se salta: el tramo no tendría longitud.
    let joint = foot_at(clamped.lp, &arc, planes, tangents, path);
    let skip = clamped.lp + 1.0e-3 * e0;
    let first = (0..=steps).find(|&i| arc[i] > skip).unwrap_or(steps);
    let mut face_stations = vec![Station {
        conformal: joint.plane.conformal,
        theta: joint.plane.theta,
        e: along.thickness(joint.plane.conformal),
        r: joint.r,
    }];
    let mut face_planes = vec![joint.plane];
    let mut face_tangents = vec![joint.tangent];
    face_stations.extend_from_slice(&stations[first..]);
    face_planes.extend_from_slice(&planes[first..]);
    face_tangents.extend_from_slice(&tangents[first..]);
    Ok(Nose {
        profile,
        rows,
        stations: face_stations,
        planes: face_planes,
        tangents: face_tangents,
        end: steps - first + 1,
        notices,
    })
}

/// Las filas [presión, medio, succión] de la nariz, de la punta a la unión,
/// con las caras ya construidas. La última fila es la primera estación de las
/// caras, bit a bit. `Err(FacesCross)` si una cara de la nariz retrocede.
fn nose_rows(
    index: usize,
    nose: &Nose,
    pressure: &SolidPoint,
    suction: &SolidPoint,
    along: &Along,
    path: &Path,
) -> Result<Vec<[SolidPoint; 3]>, BladeSolidError> {
    let mut out: Vec<[SolidPoint; 3]> = Vec::with_capacity(NOSE_STEPS + 1);
    for (foot, e, s) in &nose.rows {
        let p = nose_offset(1.0, foot, *s, *e, &nose.profile, along, path);
        let q = nose_offset(-1.0, foot, *s, *e, &nose.profile, along, path);
        let (p, q) = (solid_point(p, path), solid_point(q, path));
        out.push([p, mid_point(&p, &q, path), q]);
    }
    out.push([*pressure, mid_point(pressure, suction, path), *suction]);
    // Cada cara avanza a lo largo de la línea media de fila en fila.
    for (k, (foot, _, _)) in nose.rows.iter().enumerate() {
        for side in [0, 2] {
            let (a, b) = (out[k][side], out[k + 1][side]);
            let along = (b.m_conformal - a.m_conformal) * foot.tangent[0]
                + (b.theta - a.theta) * foot.tangent[1];
            if along <= 0.0 {
                return Err(BladeSolidError::FacesCross {
                    line: index,
                    radius: foot.r,
                    at: [foot.plane.conformal, foot.plane.theta],
                });
            }
        }
    }
    Ok(out)
}

/// Las caras de presión y succión de cada línea de corriente.
///
/// `lines` son las líneas de corriente de las que salió `camber`, en mm;
/// `curves`, la curva e(x) de cubo y tapa; `edges`, los bordes (con
/// [`LeadingKind::Blunt`], romo, nada cambia) y `same`, el interruptor «cubo =
/// tapa» de la curva; `blade_count`, Z. Los errores, en este orden:
/// [`BladeSolidError::LineMismatch`], `InclinedOutlet`, `NoseOutOfRange`,
/// `FacesCross` y `NeighboursTouch`.
pub fn blade_solid(
    camber: &BladeCamber,
    lines: &[Streamline],
    curves: &ThicknessCurves,
    edges: &BladeEdges,
    same: bool,
    blade_count: u32,
) -> Result<BladeSolid, BladeSolidError> {
    if lines.len() < 2
        || lines.len() != camber.lines.len()
        || camber
            .lines
            .iter()
            .zip(lines)
            .any(|(line, stream)| line.points.len() < 2 || (line.psi - stream.psi).abs() > 1.0e-9)
    {
        return Err(BladeSolidError::LineMismatch);
    }
    let paths: Vec<Path> = lines
        .iter()
        .map(Path::new)
        .collect::<Option<_>>()
        .ok_or(BladeSolidError::LineMismatch)?;

    let outlet = |line: &crate::blade_camber::CamberLine| line.points[line.points.len() - 1].r;
    let hub_outlet = outlet(&camber.lines[0]);
    for (index, line) in camber.lines.iter().enumerate() {
        if (outlet(line) - hub_outlet).abs() > RADIAL_OUTLET_TOLERANCE {
            return Err(BladeSolidError::InclinedOutlet {
                line: index,
                radius: outlet(line),
            });
        }
    }

    let mut solid_lines = Vec::with_capacity(lines.len());
    let mut node_faces: Vec<(Vec<SolidPoint>, Vec<SolidPoint>)> = Vec::with_capacity(lines.len());
    let mut nose_notices: Vec<SolidNotice> = Vec::new();
    for (index, (line, path)) in camber.lines.iter().zip(paths).enumerate() {
        let thickness = |x: f64| curves.at(line.psi, x);
        let nodes = &line.points;
        let path = path.with_table(nodes, thickness(0.0), thickness(1.0));
        let steps = nodes.len() - 1;

        // Estaciones: los nodos y la línea media prolongada más allá de x = 1.
        let end = &nodes[steps];
        let mut stations: Vec<Station> = nodes
            .iter()
            .map(|point| Station {
                conformal: point.m_conformal,
                theta: point.theta,
                e: thickness(point.x),
                r: point.r,
            })
            .collect();
        let tan_beta = end.beta.tan();
        // La tabla es: prolongación aguas arriba, los nodos, aguas abajo.
        let downstream = SOLID_EXTENSION_STEPS + nodes.len();
        for i in 1..=SOLID_EXTENSION_STEPS {
            let m = end.m + thickness(1.0) * i as f64 / SOLID_EXTENSION_STEPS as f64;
            let conformal = path.table_conformal[downstream + i - 1];
            stations.push(Station {
                conformal,
                theta: end.theta + (conformal - end.m_conformal) / tan_beta,
                e: thickness(1.0),
                r: path.position(m)[1],
            });
        }
        let planes: Vec<Plane> = stations
            .iter()
            .map(|station| Plane {
                conformal: station.conformal,
                theta: station.theta,
            })
            .collect();
        let tangents = tangents(&planes);

        let along = Along {
            nodes: nodes.iter().map(|p| (p.m_conformal, p.x)).collect(),
            thickness: Box::new(thickness),
        };
        // La nariz: con romo no hay, y las caras parten del nodo 0 con el canto
        // perpendicular. Con elipse o afilado parten de la unión, que se
        // desplaza como el resto de las caras.
        let nose = if edges.leading == LeadingKind::Blunt {
            None
        } else {
            let dims = edges.profile_at(line.psi, same).dims;
            Some(plan_nose(
                index,
                dims,
                edges.leading,
                &path,
                nodes,
                &stations,
                &planes,
                &tangents,
                &along,
                thickness(0.0),
            )?)
        };
        let (face_stations, face_planes, face_tangents, face_end, first) = match &nose {
            Some(nose) => (
                &nose.stations[..],
                &nose.planes[..],
                &nose.tangents[..],
                nose.end,
                Mode::Face,
            ),
            None => (&stations[..], &planes[..], &tangents[..], steps, Mode::Foot),
        };
        let pressure = face(
            index,
            &path,
            face_stations,
            face_planes,
            face_tangents,
            &along,
            1.0,
            face_end,
            steps,
            first,
        )?;
        let suction = face(
            index,
            &path,
            face_stations,
            face_planes,
            face_tangents,
            &along,
            -1.0,
            face_end,
            steps,
            first,
        )?;
        node_faces.push((
            nominal_sections(&path, &stations, &planes, &tangents, &along, 1.0, steps),
            nominal_sections(&path, &stations, &planes, &tangents, &along, -1.0, steps),
        ));
        let nose_rows = match &nose {
            Some(nose) => nose_rows(index, nose, &pressure[0], &suction[0], &along, &path)?,
            None => Vec::new(),
        };
        if let Some(nose) = nose {
            nose_notices.extend(nose.notices);
        }
        let middle = pressure
            .iter()
            .zip(&suction)
            .map(|(p, s)| mid_point(p, s, &path))
            .collect();
        solid_lines.push(SolidLine {
            psi: line.psi,
            pressure,
            middle,
            suction,
            nose: nose_rows,
            true_thickness: Vec::new(),
        });
    }

    // Vecinos: el espesor angular de un álabe no llega al paso 2π/Z.
    let pitch = 2.0 * std::f64::consts::PI / f64::from(blade_count);
    for (index, line) in solid_lines.iter().enumerate() {
        for point in &line.pressure {
            let gap = theta_at(&line.suction, point.m_conformal) - point.theta;
            if gap >= pitch {
                return Err(BladeSolidError::NeighboursTouch {
                    line: index,
                    radius: point.r,
                    at: [point.m_conformal, point.theta],
                });
            }
        }
    }

    // Lecturas y avisos.
    for (index, line) in solid_lines.iter_mut().enumerate() {
        line.true_thickness = true_thickness(camber, index, &node_faces[index]);
    }
    let readings = readings(camber, &solid_lines, blade_count);
    let mut notices = notices(camber, &solid_lines);
    notices.extend(nose_notices);
    Ok(BladeSolid {
        lines: solid_lines,
        readings,
        notices,
    })
}

/// (x, y, z) del punto, m.
fn cartesian(point: &SolidPoint) -> [f64; 3] {
    [
        point.r * point.theta.cos(),
        point.r * point.theta.sin(),
        point.z,
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// El espesor real en cada nodo de la línea `index`, m: |(P⁺ − P⁻)·N| con P±
/// los puntos desplazados del nodo antes del recorte, en cartesianas, y N la
/// normal unitaria a la superficie media: el producto vectorial de la
/// tangente a lo largo (diferencias de nodos) por la tangente a lo ancho (del
/// nodo a su homólogo en la línea vecina; en la tapa, la anterior). Si no hay
/// normal (tangentes paralelas), la distancia entre los dos puntos.
fn true_thickness(
    camber: &BladeCamber,
    index: usize,
    faces: &(Vec<SolidPoint>, Vec<SolidPoint>),
) -> Vec<f64> {
    let node = |line: usize, j: usize| {
        let p = &camber.lines[line].points[j];
        [p.r * p.theta.cos(), p.r * p.theta.sin(), p.z]
    };
    let neighbour = if index + 1 < camber.lines.len() {
        index + 1
    } else {
        index - 1
    };
    let last = camber.lines[index].points.len() - 1;
    (0..=last)
        .map(|j| {
            let gap = sub(cartesian(&faces.0[j]), cartesian(&faces.1[j]));
            let along = sub(
                node(index, (j + 1).min(last)),
                node(index, j.saturating_sub(1)),
            );
            let across = sub(node(neighbour, j), node(index, j));
            let normal = cross(along, across);
            let length = norm(normal);
            if length > 0.0 {
                dot(gap, normal).abs() / length
            } else {
                norm(gap)
            }
        })
        .collect()
}

/// Las x a que se lee el espesor real, el canto y la garganta: entrada, un
/// tercio, dos tercios y salida.
pub const READING_STATIONS: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];

/// La garganta de entrada de una línea: la distancia más corta, en 3D, entre
/// el borde de ataque de un álabe y la cara de succión del vecino.
#[derive(Debug, Clone, PartialEq)]
pub struct Throat {
    /// m.
    pub width: f64,
    /// El primer punto de la cara de presión del álabe vecino (θ + 2π/Z).
    pub from: SolidPoint,
    /// El punto de la cara de succión más cercano a `from`.
    pub to: SolidPoint,
}

/// Lo que se lee del álabe con espesor. De geometría: sin criterio en esta
/// etapa.
#[derive(Debug, Clone, PartialEq)]
pub struct SolidReadings {
    /// Espesor real del cubo y de la tapa en [`READING_STATIONS`], m.
    pub true_thickness_hub: [f64; 4],
    pub true_thickness_shroud: [f64; 4],
    /// Una por línea; `None` con un solo álabe, que no tiene vecino.
    pub throats: Vec<Option<Throat>>,
    /// El canto de salida de cada línea, r·(θ_succión − θ_presión), m.
    pub exit_edges: Vec<f64>,
    /// τ2 con el canto de la línea central: la forma de
    /// [`crate::blade_blockage`].
    pub exit_blockage: f64,
}

/// Un aviso: una lectura bajo el mínimo de fundición. `at` es (m′, θ) del
/// nodo de la línea media.
#[derive(Debug, Clone, PartialEq)]
pub struct SolidNotice {
    pub line: usize,
    pub at: [f64; 2],
    pub message: String,
}

/// El nodo de la línea más cercano a esa x.
fn node_at(steps: usize, x: f64) -> usize {
    ((x * steps as f64).round() as usize).min(steps)
}

/// Un número con coma decimal.
fn comma(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

fn readings(camber: &BladeCamber, lines: &[SolidLine], blade_count: u32) -> SolidReadings {
    let steps = lines[0].pressure.len() - 1;
    let at_stations =
        |line: &SolidLine| READING_STATIONS.map(|x| line.true_thickness[node_at(steps, x)]);

    let pitch = 2.0 * std::f64::consts::PI / f64::from(blade_count);
    let throats = lines
        .iter()
        .map(|line| (blade_count > 1).then(|| throat(line, pitch)))
        .collect();
    let exit_edges: Vec<f64> = lines
        .iter()
        .map(|line| {
            let [pressure, _, suction] = line.trailing_edge();
            pressure.r * (suction.theta - pressure.theta)
        })
        .collect();

    // La línea central: la de ψ más cercano a 0,5, la primera si hay empate.
    let mid = (0..lines.len())
        .min_by(|a, b| {
            (camber.lines[*a].psi - 0.5)
                .abs()
                .total_cmp(&(camber.lines[*b].psi - 0.5).abs())
        })
        .unwrap_or(0);
    let outlet_diameter = 2.0 * camber.lines[mid].points[steps].r;
    let ratio = f64::from(blade_count) * exit_edges[mid] / (std::f64::consts::PI * outlet_diameter);
    SolidReadings {
        true_thickness_hub: at_stations(&lines[0]),
        true_thickness_shroud: at_stations(&lines[lines.len() - 1]),
        throats,
        exit_edges,
        exit_blockage: 1.0 / (1.0 - ratio.min(0.6)),
    }
}

/// La garganta de entrada de `line`: del primer punto de la cara de presión
/// del vecino a la polilínea de succión de ésta.
fn throat(line: &SolidLine, pitch: f64) -> Throat {
    // Con nariz, la garganta parte de la punta del vecino; con romo, de la
    // esquina de presión de su canto, como en la etapa 1.
    let first = if line.nose.is_empty() {
        line.pressure[0]
    } else {
        line.leading_tip()
    };
    let from = SolidPoint {
        theta: first.theta + pitch,
        ..first
    };
    // La cara de succión entera: la de la nariz (hasta la unión) y el cuerpo.
    let suction: Vec<SolidPoint> = line
        .nose
        .iter()
        .map(|row| row[2])
        .chain(
            line.suction
                .iter()
                .skip(usize::from(!line.nose.is_empty()))
                .copied(),
        )
        .collect();
    let target = cartesian(&from);
    let mut best: Option<(f64, SolidPoint)> = None;
    for pair in suction.windows(2) {
        let (a, b) = (cartesian(&pair[0]), cartesian(&pair[1]));
        let d = sub(b, a);
        let squared = dot(d, d);
        let t = if squared > 0.0 {
            (dot(sub(target, a), d) / squared).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let foot = [a[0] + t * d[0], a[1] + t * d[1], a[2] + t * d[2]];
        let distance = norm(sub(target, foot));
        if best
            .as_ref()
            .is_none_or(|(shortest, _)| distance < *shortest)
        {
            best = Some((
                distance,
                SolidPoint {
                    z: foot[2],
                    r: foot[0].hypot(foot[1]),
                    theta: foot[1].atan2(foot[0]),
                    m_conformal: pair[0].m_conformal
                        + t * (pair[1].m_conformal - pair[0].m_conformal),
                },
            ));
        }
    }
    let (width, to) = best.expect("la cara de succión tiene al menos dos puntos");
    // θ de `to` en la misma vuelta que la de `from`.
    let turns = ((from.theta - to.theta) / (2.0 * std::f64::consts::PI)).round();
    let to = SolidPoint {
        theta: to.theta + turns * 2.0 * std::f64::consts::PI,
        ..to
    };
    Throat { width, from, to }
}

/// Un aviso por cada lectura de espesor real, en cualquier línea, bajo el
/// mínimo de fundición del nodo ([`crate::blade_thickness::node_floor`]).
fn notices(camber: &BladeCamber, lines: &[SolidLine]) -> Vec<SolidNotice> {
    let steps = lines[0].pressure.len() - 1;
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        for x in READING_STATIONS {
            let node = node_at(steps, x);
            let real = line.true_thickness[node];
            let floor = crate::blade_thickness::node_floor(x);
            if real < floor {
                let point = &camber.lines[index].points[node];
                // «3» y «1,5»: el mínimo sin ceros de sobra.
                let minimum = comma(floor * 1000.0, 1).replace(",0", "");
                out.push(SolidNotice {
                    line: index,
                    at: [point.m_conformal, point.theta],
                    message: format!(
                        "línea de corriente {index} (0 es el cubo): el espesor real en x = {} \
                         es {} mm, menor que el mínimo de fundición de {minimum} mm",
                        comma(x, 2),
                        comma(real * 1000.0, 2),
                    ),
                });
            }
        }
    }
    out
}

/// El texto de un error del diseño, sin el «entrada inválida» de `Display`.
fn reason(error: DesignError) -> String {
    match error {
        DesignError::Input(text) => text,
        other => other.to_string(),
    }
}

/// El álabe con espesor de un diseño: encadena la línea media
/// ([`camber_for_design`], la misma que dibuja el visor y escribe el archivo),
/// las líneas de corriente, las curvas de espesor del `setup` y
/// [`blade_solid`]. Con una forma 2D, o si algo de lo anterior falla, no hay
/// álabe con espesor y el error es [`BladeSolidError::NoCamber`] con su texto.
pub fn blade_solid_for_design(
    result: &DesignResult,
    impeller: &ImpellerGeometry,
    model: &MeridionalModel,
    setup: &BladeSetup,
    incidence_deg: f64,
) -> Result<BladeSolid, BladeSolidError> {
    let camber = camber_for_design(result, impeller, model, setup, incidence_deg)
        .map_err(|error| BladeSolidError::NoCamber(reason(error)))?
        .ok_or_else(|| {
            BladeSolidError::NoCamber(
                "la forma del álabe es 2D: su espesor llega en la etapa 3".to_string(),
            )
        })?;
    let lines = streamlines(model, setup.streamlines)
        .map_err(|error| BladeSolidError::NoCamber(error.to_string()))?;
    let curves = thickness_curves_for_design(result, &camber, &setup.thickness)
        .map_err(|error| BladeSolidError::NoCamber(reason(error)))?;
    blade_solid(
        &camber,
        &lines,
        &curves,
        &setup.edges,
        setup.thickness.same,
        result.selection.blade_count,
    )
}
