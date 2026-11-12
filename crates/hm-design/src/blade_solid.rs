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
    TailProfile, TrailingDims, TrailingKind,
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

/// Pasos de la salida con forma, de la unión a D2: 16, como la nariz (la
/// elipse a ángulo uniforme, más densa hacia D2). De hmflow.
pub const TAIL_STEPS: usize = 16;

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
    /// Filas [presión, medio, succión] de la unión de la salida a D2:
    /// [`TAIL_STEPS`] + 1, la primera igual a la última de `pressure`,
    /// `middle` y `suction`, bit a bit. Con una salida con forma, las dos
    /// caras acaban en esa unión. Vacía con recortada.
    pub tail: Vec<[SolidPoint; 3]>,
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

    /// El canto de salida en D2: los de la última estación o, con una salida
    /// con forma, la última fila de `tail`.
    #[must_use]
    pub fn trailing_edge(&self) -> [SolidPoint; 3] {
        if let Some(row) = self.tail.last() {
            return *row;
        }
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

    /// La salida con forma no cabe en la línea ni con la cota acotada, o su
    /// cota no es un número finito. `radius`, `length` y `limit` en m.
    #[error(
        "línea de corriente {line} (0 es el cubo): la salida no cabe en la línea en r = {} mm: longitud = {} mm y el máximo es {} mm",
        mm(.radius),
        mm(.length),
        mm(.limit)
    )]
    TailOutOfRange {
        line: usize,
        radius: f64,
        length: f64,
        limit: f64,
    },

    /// La nariz y la salida con forma no caben en la misma línea. `radius`
    /// en m.
    #[error(
        "línea de corriente {line} (0 es el cubo): la nariz y la salida se solapan en r = {} mm: Lp más la longitud de la salida pasan de la longitud de la línea",
        mm(.radius)
    )]
    NoseAndExitOverlap { line: usize, radius: f64 },

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
    /// La salida con forma de esta línea, si la hay.
    tail: Option<TailShape>,
}

/// La salida con forma de una línea: su perfil y lo que hace falta para saber
/// a qué distancia de D2 cae una m′.
struct TailShape {
    profile: TailProfile,
    /// e(1) de la línea.
    e1: f64,
    /// (m′, arco desde la punta) de las estaciones 0..=J de la línea media.
    arcs: Vec<(f64, f64)>,
}

impl TailShape {
    /// La distancia a D2 sobre la línea media de la m′ dada, m; no positiva
    /// desde D2 hacia fuera.
    fn distance_to_exit(&self, conformal: f64) -> f64 {
        let last = self.arcs.len() - 1;
        let total = self.arcs[last].1;
        let at = if conformal <= self.arcs[0].0 {
            0.0
        } else if conformal >= self.arcs[last].0 {
            total
        } else {
            let segment = (self.arcs.partition_point(|node| node.0 <= conformal) - 1).min(last - 1);
            let (a, b) = (self.arcs[segment], self.arcs[segment + 1]);
            a.1 + (conformal - a.0) / (b.0 - a.0) * (b.1 - a.1)
        };
        total - at
    }
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

    /// El semiespesor de la cara de `sign` (+1 presión, −1 succión) en la m′
    /// dada: e/2, o el de la salida con forma si el punto cae en ella.
    fn half(&self, sign: f64, conformal: f64) -> f64 {
        let e = self.thickness(conformal);
        match &self.tail {
            None => 0.5 * e,
            Some(tail) => {
                let v = tail.distance_to_exit(conformal);
                let (pressure, suction) = tail.profile.half_thicknesses(v, e, tail.e1);
                if sign > 0.0 { pressure } else { suction }
            }
        }
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
    let displaced = |half: f64, r: f64| {
        let shift = sign * half / r;
        Plane {
            conformal: foot.conformal + shift * t[1],
            theta: foot.theta - shift * t[0],
        }
    };
    let mut point = displaced(0.5 * e, r);
    if mode != Mode::Foot {
        for _ in 0..FIXED_POINT_PASSES {
            let middle = 0.5 * (foot.conformal + point.conformal);
            let half = if mode == Mode::Face {
                along.half(sign, point.conformal)
            } else {
                0.5 * e
            };
            point = displaced(half, path.at(middle)[1]);
        }
    }
    point
}

/// Cómo se reparten los puntos de una cara a lo largo de m′.
#[derive(Clone, Copy)]
enum Grid<'a> {
    /// J + 1 puntos a pasos iguales de m′ del primero hasta `end`.
    Equal(usize),
    /// Exactamente estas m′; la última es `end`, donde la cara se corta.
    Targets(&'a [f64]),
}

/// Una cara: la polilínea desplazada de la línea media, cortada en m′ = `end`
/// y remuestreada según `grid`. `first` es cómo se desplaza la estación 0:
/// [`Mode::Foot`] en el canto romo y [`Mode::Face`] en la unión de la nariz.
#[allow(clippy::too_many_arguments)]
fn face(
    line: usize,
    path: &Path,
    stations: &[Station],
    planes: &[Plane],
    tangents: &[[f64; 2]],
    along: &Along,
    sign: f64,
    end: f64,
    grid: Grid,
    first: Mode,
) -> Result<Vec<SolidPoint>, BladeSolidError> {
    let shifted = shift_stations(path, stations, planes, tangents, along, sign, first);
    face_from_shifted(
        line, path, stations, planes, tangents, along, sign, end, grid, &shifted,
    )
}

/// El punto desplazado de cada estación (la parte cara de [`face`]): solo
/// depende de las estaciones, del signo y de `along`, no del corte ni del
/// muestreo, así que varias caras de la misma línea la comparten.
fn shift_stations(
    path: &Path,
    stations: &[Station],
    planes: &[Plane],
    tangents: &[[f64; 2]],
    along: &Along,
    sign: f64,
    first: Mode,
) -> Vec<Plane> {
    stations
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
        .collect()
}

/// Corta y remuestrea unos puntos ya desplazados ([`shift_stations`] de las
/// mismas `stations`, `planes` y `tangents`): el resto de [`face`].
#[allow(clippy::too_many_arguments)]
fn face_from_shifted(
    line: usize,
    path: &Path,
    stations: &[Station],
    planes: &[Plane],
    tangents: &[[f64; 2]],
    along: &Along,
    sign: f64,
    end: f64,
    grid: Grid,
    shifted: &[Plane],
) -> Result<Vec<SolidPoint>, BladeSolidError> {
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
    let count = match grid {
        Grid::Equal(intervals) => intervals + 1,
        Grid::Targets(targets) => targets.len(),
    };
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let last = k + 1 == count;
        let target = match grid {
            Grid::Equal(intervals) => {
                if last {
                    end
                } else {
                    start + (end - start) * k as f64 / intervals as f64
                }
            }
            Grid::Targets(targets) => targets[k],
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
            theta: if last {
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

/// El punto de una cara que sale del pie `foot` de la línea media: ±h/r a lo
/// largo de su normal (t_θ, −t_m′), con + hacia la presión, con h el
/// semiespesor de esa cara que da `half_of(e)` y r a medio desplazamiento.
/// Como en [`Mode::Face`], e se toma de la x del punto desplazado y no de la
/// del pie: así la nariz y la salida, que siguen con ese mismo criterio, se
/// unen sin escalón con las caras aunque e(x) tenga pendiente.
fn shaped_offset(
    sign: f64,
    foot: &Foot,
    e_foot: f64,
    half_of: &dyn Fn(f64) -> (f64, f64),
    along: &Along,
    path: &Path,
) -> Plane {
    let half = |e: f64| {
        let (pressure, suction) = half_of(e);
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

/// El punto de la nariz a la distancia `s` de la punta ([`shaped_offset`] con
/// el perfil de la nariz).
fn nose_offset(
    sign: f64,
    foot: &Foot,
    s: f64,
    e_foot: f64,
    profile: &NoseProfile,
    along: &Along,
    path: &Path,
) -> Plane {
    shaped_offset(
        sign,
        foot,
        e_foot,
        &|e| profile.half_thicknesses(s, e),
        along,
        path,
    )
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

/// La salida con forma de una línea ya planeada.
struct Tail {
    profile: TailProfile,
    e1: f64,
    /// m′ de la unión: el plano donde acaban las dos caras.
    joint_conformal: f64,
    /// Las m′ de las filas de la unión (la primera) a D2 (la última): a ángulo
    /// de elipse uniforme, más densas hacia D2.
    targets: Vec<f64>,
    /// (m′, arco) de las estaciones 0..=J, para saber la distancia a D2.
    arcs: Vec<(f64, f64)>,
    notices: Vec<SolidNotice>,
}

/// Acota las cotas de la salida de la línea `index` y localiza la unión a
/// L − longitud. `Err` si la nariz y la salida no caben juntas (`nose_lp` es
/// la Lp ya acotada, si hay nariz), si la salida no cabe ni acotada o si la
/// cota no es finita (`TailOutOfRange`).
#[allow(clippy::too_many_arguments)]
fn plan_tail(
    index: usize,
    dims: TrailingDims,
    kind: TrailingKind,
    path: &Path,
    nodes: &[CamberPoint],
    planes: &[Plane],
    tangents: &[[f64; 2]],
    e1: f64,
    nose_lp: f64,
) -> Result<Tail, BladeSolidError> {
    let steps = nodes.len() - 1;
    let arc = arcs(path, planes, steps);
    let total = arc[steps];
    let exit_r = nodes[steps].r;
    let floor = crate::blade_thickness::node_floor(1.0);
    let limit = crate::blade_edges::TAIL_MAX_FRACTION * total;
    let out_of_range = |length: f64| BladeSolidError::TailOutOfRange {
        line: index,
        radius: exit_r,
        length,
        limit,
    };
    let clamped = dims
        .clamped(e1, total, floor)
        .ok_or_else(|| out_of_range(dims.length))?;
    // Una línea tan corta que 0,3·L < e(1)/2 deja ganar el mínimo e(1)/2:
    // como con la nariz, la salida que no cabe es un error, no se acorta aquí
    // en silencio.
    if clamped.length > limit {
        return Err(out_of_range(clamped.length));
    }
    if nose_lp + clamped.length > total {
        return Err(BladeSolidError::NoseAndExitOverlap {
            line: index,
            radius: exit_r,
        });
    }
    let at = [nodes[steps].m_conformal, nodes[steps].theta];
    let line = |text: String| SolidNotice {
        line: index,
        at,
        message: format!("línea de corriente {index} (0 es el cubo): {text}"),
    };
    let mut notices = Vec::new();
    if clamped.length != dims.length {
        notices.push(line(format!(
            "la longitud de la salida {} mm se acotó a {} mm (entre e(1)/2 = {} mm y 0,3·L = {} mm)",
            comma(dims.length * 1000.0, 2),
            comma(clamped.length * 1000.0, 2),
            comma(e1 / 2.0 * 1000.0, 2),
            comma(crate::blade_edges::TAIL_MAX_FRACTION * total * 1000.0, 2),
        )));
    }
    let minimum = comma(floor * 1000.0, 1).replace(",0", "");
    if kind != TrailingKind::Ellipse && clamped.end != dims.end {
        notices.push(line(format!(
            "el espesor final del limado {} mm se acotó a {} mm (entre el mínimo de fundición de {minimum} mm y e(1) = {} mm)",
            comma(dims.end * 1000.0, 2),
            comma(clamped.end * 1000.0, 2),
            comma(e1 * 1000.0, 2),
        )));
    }
    if kind == TrailingKind::Ellipse {
        notices.push(line(format!(
            "la elipse de salida llega a espesor 0 en D2, bajo el mínimo de fundición de {minimum} mm"
        )));
    }
    let joint = foot_at(total - clamped.length, &arc, planes, tangents, path);
    let (joint_conformal, d2) = (joint.plane.conformal, planes[steps].conformal);
    // Filas a ángulo de elipse uniforme: la distancia a D2 es
    // (1 − cos α) de la longitud, con α de 90° (la unión) a 0° (D2).
    let targets = (0..=TAIL_STEPS)
        .map(|k| {
            if k == 0 {
                joint_conformal
            } else if k == TAIL_STEPS {
                d2
            } else {
                let alpha = std::f64::consts::FRAC_PI_2 * (1.0 - k as f64 / TAIL_STEPS as f64);
                d2 - (1.0 - alpha.cos()) * (d2 - joint_conformal)
            }
        })
        .collect();
    Ok(Tail {
        profile: TailProfile {
            kind,
            dims: clamped,
        },
        e1,
        joint_conformal,
        targets,
        arcs: (0..=steps).map(|i| (planes[i].conformal, arc[i])).collect(),
        notices,
    })
}

/// Subdivisiones de cada tramo de la línea media desde la unión de la salida.
const TAIL_SUBDIVISIONS: usize = 8;

/// Las listas de estaciones con cada tramo posterior a `from` (una m′) partido
/// en [`TAIL_SUBDIVISIONS`]: la salida con forma, sobre todo la elipse, no se
/// resuelve con estaciones separadas más que ella. Las nuevas se interpolan
/// como [`foot_at`] (plano y tangente lineales entre estaciones).
fn refine_from(
    from: f64,
    stations: &[Station],
    planes: &[Plane],
    tangents: &[[f64; 2]],
    along: &Along,
    path: &Path,
) -> (Vec<Station>, Vec<Plane>, Vec<[f64; 2]>) {
    let (mut out_stations, mut out_planes, mut out_tangents) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..planes.len() {
        out_stations.push(stations[i]);
        out_planes.push(planes[i]);
        out_tangents.push(tangents[i]);
        if i + 1 == planes.len() || planes[i + 1].conformal <= from {
            continue;
        }
        for k in 1..TAIL_SUBDIVISIONS {
            let u = k as f64 / TAIL_SUBDIVISIONS as f64;
            let (a, b) = (planes[i], planes[i + 1]);
            let plane = Plane {
                conformal: a.conformal + u * (b.conformal - a.conformal),
                theta: a.theta + u * (b.theta - a.theta),
            };
            let t = [
                tangents[i][0] + u * (tangents[i + 1][0] - tangents[i][0]),
                tangents[i][1] + u * (tangents[i + 1][1] - tangents[i][1]),
            ];
            let norm = t[0].hypot(t[1]);
            let tangent = if norm > 0.0 {
                [t[0] / norm, t[1] / norm]
            } else {
                tangents[i]
            };
            let r = path.at(plane.conformal)[1];
            out_stations.push(Station {
                conformal: plane.conformal,
                theta: plane.theta,
                e: along.thickness(plane.conformal),
                r,
            });
            out_planes.push(plane);
            out_tangents.push(tangent);
        }
    }
    (out_stations, out_planes, out_tangents)
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
    // Por línea: el corte nominal de D2 (el del espesor del cuerpo) y la cara
    // de presión entera hasta D2.
    let mut nominal_exits: Vec<([SolidPoint; 3], Vec<SolidPoint>)> =
        Vec::with_capacity(lines.len());
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
            tail: None,
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
        // Las caras hasta D2 con el espesor del cuerpo: con recortada son las
        // caras; con salida con forma, el corte nominal de D2 (de donde salen
        // el canto y el bloqueo del 1D) y la referencia de a2.
        let d2_end = face_planes[face_end].conformal;
        // El desplazamiento de las estaciones, una vez por cara: las caras
        // recortadas y las del cuerpo de la salida con forma lo comparten.
        let shift = |sign: f64| {
            shift_stations(
                &path,
                face_stations,
                face_planes,
                face_tangents,
                &along,
                sign,
                first,
            )
        };
        let (shifted_pressure, shifted_suction) = (shift(1.0), shift(-1.0));
        let cut_face = |sign: f64, shifted: &[Plane], end: f64| {
            face_from_shifted(
                index,
                &path,
                face_stations,
                face_planes,
                face_tangents,
                &along,
                sign,
                end,
                Grid::Equal(steps),
                shifted,
            )
        };
        let cut_pressure = cut_face(1.0, &shifted_pressure, d2_end)?;
        let cut_suction = cut_face(-1.0, &shifted_suction, d2_end)?;
        let cut_exit = [
            *cut_pressure.last().expect("la cara tiene puntos"),
            mid_point(
                cut_pressure.last().expect("la cara tiene puntos"),
                cut_suction.last().expect("la cara tiene puntos"),
                &path,
            ),
            *cut_suction.last().expect("la cara tiene puntos"),
        ];
        // La salida con forma: las caras acaban en la unión y la salida sigue
        // en `tail`.
        let tail = if edges.trailing == TrailingKind::Cut {
            None
        } else {
            let nose_lp = nose.as_ref().map_or(0.0, |nose| nose.profile.dims.lp);
            Some(plan_tail(
                index,
                edges.tail_at(line.psi, same).dims,
                edges.trailing,
                &path,
                nodes,
                &planes,
                &tangents,
                thickness(1.0),
                nose_lp,
            )?)
        };
        let (pressure, suction, tail_rows) = match &tail {
            None => (cut_pressure.clone(), cut_suction.clone(), Vec::new()),
            Some(tail) => {
                let (pressure, suction) = (
                    cut_face(1.0, &shifted_pressure, tail.joint_conformal)?,
                    cut_face(-1.0, &shifted_suction, tail.joint_conformal)?,
                );
                let shaped = Along {
                    nodes: nodes.iter().map(|p| (p.m_conformal, p.x)).collect(),
                    thickness: Box::new(thickness),
                    tail: Some(TailShape {
                        profile: tail.profile,
                        e1: tail.e1,
                        arcs: tail.arcs.clone(),
                    }),
                };
                // Solo cambian las estaciones desde la unión: antes de ella la
                // salida con forma da el mismo espesor que el cuerpo (ya
                // desplazado arriba). Se parte de una estación antes de la
                // última cuyos dos puntos desplazados no pasan de la unión
                // (una cara se adelanta a su pie en m′ hasta e/2r), así la
                // primera fila de la cola cae entre dos puntos como antes.
                let from = (0..face_planes.len())
                    .rposition(|i| {
                        shifted_pressure[i]
                            .conformal
                            .max(shifted_suction[i].conformal)
                            <= tail.joint_conformal
                    })
                    .map_or(0, |i| i.saturating_sub(1));
                let (fine_stations, fine_planes, fine_tangents) = refine_from(
                    tail.joint_conformal,
                    &face_stations[from..],
                    &face_planes[from..],
                    &face_tangents[from..],
                    &shaped,
                    &path,
                );
                let rest = &tail.targets[1..];
                let side = |sign: f64| {
                    face(
                        index,
                        &path,
                        &fine_stations,
                        &fine_planes,
                        &fine_tangents,
                        &shaped,
                        sign,
                        d2_end,
                        Grid::Targets(rest),
                        if from == 0 { first } else { Mode::Face },
                    )
                };
                let (rows_p, rows_s) = (side(1.0)?, side(-1.0)?);
                let (joint_p, joint_s) = (
                    *pressure.last().expect("la cara tiene puntos"),
                    *suction.last().expect("la cara tiene puntos"),
                );
                let mut rows = vec![[joint_p, mid_point(&joint_p, &joint_s, &path), joint_s]];
                for (k, (p, q)) in rows_p.iter().zip(&rows_s).enumerate() {
                    // La punta de la elipse en D2 es un solo punto.
                    let q = if k + 1 == rows_p.len() && tail.profile.kind == TrailingKind::Ellipse {
                        p
                    } else {
                        q
                    };
                    rows.push([*p, mid_point(p, q, &path), *q]);
                }
                (pressure, suction, rows)
            }
        };
        nominal_exits.push((cut_exit, cut_pressure));
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
        if let Some(tail) = tail {
            nose_notices.extend(tail.notices);
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
            tail: tail_rows,
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
    let readings = readings(camber, &solid_lines, &nominal_exits, blade_count);
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

/// La garganta de salida de una línea: la distancia entre álabes a2 con la
/// salida recortada y con la forma elegida.
///
/// **Definición de a2** (Gülich, *Centrifugal Pumps*, 4.ª ed., 2020, Tabla
/// 0.2 y §3.3, ec. 3.8, p. 136): «la distancia entre álabes en la garganta»,
/// la distancia más corta entre el canto de salida de un álabe y la superficie
/// del álabe vecino, con βa2 = arcsen(a2/t2), t2 = π·d2/Z. Aquí: la distancia
/// más corta, en 3D, entre los puntos del canto de salida de este álabe y la
/// cara de presión del vecino (θ + 2π/Z), que es la que mira a ese canto: el
/// pie de la perpendicular cae dentro de la cara, a una distancia de
/// t2·sen β2B − e, y no en la nariz ni en el canto de D2 del otro lado (que
/// daría la cuerda del paso). Limar por succión retira material de la esquina
/// de succión del canto, la que mira a la garganta, y la abre (Gülich, 4.5.2,
/// fig. 3.6); limar por presión no toca esa esquina.
#[derive(Debug, Clone, PartialEq)]
pub struct OutletGap {
    /// a2 con la salida recortada, m.
    pub cut: f64,
    /// a2′ con la forma elegida (igual a `cut` con recortada), m.
    pub shaped: f64,
    /// El punto del canto de salida de este álabe que da a2′.
    pub from: SolidPoint,
    /// El punto de la cara de presión del álabe vecino (θ + 2π/Z) más cercano
    /// a `from`, con su θ en la misma vuelta que la de `from`.
    pub to: SolidPoint,
}

impl OutletGap {
    /// a2′/a2.
    #[must_use]
    pub fn ratio(&self) -> f64 {
        self.shaped / self.cut
    }

    /// El rango de la ganancia de altura que sugiere el limado por succión:
    /// H′/H = (a2′/a2)^x con x de 0,4 a 0,8, de modo que sale
    /// (a2′/a2)^0,4 − 1 y (a2′/a2)^0,8 − 1 (Gülich, 4.5.2, p. 207).
    #[must_use]
    pub fn head_gain_range(&self) -> (f64, f64) {
        let ratio = self.ratio();
        (ratio.powf(0.4) - 1.0, ratio.powf(0.8) - 1.0)
    }
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
    /// El canto de salida nominal de cada línea (el del espesor del cuerpo
    /// cortado en D2), r·(θ_succión − θ_presión), m. Con recortada es el
    /// canto del álabe; con salida con forma, el que sigue el 1D.
    pub exit_edges: Vec<f64>,
    /// El canto real en D2 de la forma elegida de cada línea, m: con limado,
    /// end/sen β2; con elipse, 0. Con recortada, igual a `exit_edges`.
    pub shaped_exit_edges: Vec<f64>,
    /// a2 de cada línea con la salida recortada y con la elegida; `None` con
    /// un solo álabe.
    pub outlet_gaps: Vec<Option<OutletGap>>,
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

fn readings(
    camber: &BladeCamber,
    lines: &[SolidLine],
    nominal: &[([SolidPoint; 3], Vec<SolidPoint>)],
    blade_count: u32,
) -> SolidReadings {
    let steps = lines[0].pressure.len() - 1;
    let at_stations =
        |line: &SolidLine| READING_STATIONS.map(|x| line.true_thickness[node_at(steps, x)]);

    let pitch = 2.0 * std::f64::consts::PI / f64::from(blade_count);
    let throats = lines
        .iter()
        .map(|line| (blade_count > 1).then(|| throat(line, pitch)))
        .collect();
    let edge = |row: &[SolidPoint; 3]| row[0].r * (row[2].theta - row[0].theta);
    // El nominal: el corte de D2 con el espesor del cuerpo; el 1D sigue con él.
    let exit_edges: Vec<f64> = nominal.iter().map(|(exit, _)| edge(exit)).collect();
    let shaped_exit_edges: Vec<f64> = lines
        .iter()
        .map(|line| edge(&line.trailing_edge()))
        .collect();
    let outlet_gaps = lines
        .iter()
        .zip(nominal)
        .map(|(line, (exit, cut_pressure))| {
            (blade_count > 1).then(|| outlet_gap(line, exit, cut_pressure, pitch))
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
        shaped_exit_edges,
        outlet_gaps,
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

/// El punto de `line` más cercano a `target` (cartesianas, m) y la distancia.
fn nearest_on(target: [f64; 3], line: &[SolidPoint]) -> (f64, SolidPoint) {
    let mut best: Option<(f64, SolidPoint)> = None;
    for pair in line.windows(2) {
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
    best.expect("la cara tiene al menos dos puntos")
}

/// a2 de `line`: la distancia más corta entre su canto de salida y la cara de
/// presión del álabe vecino (θ + 2π/Z), con la salida recortada (`cut_exit`,
/// `cut_pressure`) y con la elegida (la de `line`). La cara de presión del
/// vecino es la de esta línea girada un paso, y las distancias son las mismas
/// que con el canto del vecino girado un paso atrás.
fn outlet_gap(
    line: &SolidLine,
    cut_exit: &[SolidPoint; 3],
    cut_pressure: &[SolidPoint],
    pitch: f64,
) -> OutletGap {
    let measure = |exit: &[SolidPoint; 3], pressure: &[SolidPoint]| {
        let neighbour: Vec<SolidPoint> = pressure
            .iter()
            .map(|point| SolidPoint {
                theta: point.theta + pitch,
                ..*point
            })
            .collect();
        let mut best: Option<(f64, SolidPoint, SolidPoint)> = None;
        for from in exit {
            let (distance, to) = nearest_on(cartesian(from), &neighbour);
            if best
                .as_ref()
                .is_none_or(|(shortest, ..)| distance < *shortest)
            {
                // θ de `to` en la misma vuelta que la de `from`.
                let turns = ((from.theta - to.theta) / (2.0 * std::f64::consts::PI)).round();
                let to = SolidPoint {
                    theta: to.theta + turns * 2.0 * std::f64::consts::PI,
                    ..to
                };
                best = Some((distance, *from, to));
            }
        }
        best.expect("el canto tiene tres puntos")
    };
    let shaped_pressure: Vec<SolidPoint> = line
        .pressure
        .iter()
        .copied()
        .chain(line.tail.iter().skip(1).map(|row| row[0]))
        .collect();
    let (cut, ..) = measure(cut_exit, cut_pressure);
    let (shaped, from, to) = measure(&line.trailing_edge(), &shaped_pressure);
    OutletGap {
        cut,
        shaped,
        from,
        to,
    }
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
