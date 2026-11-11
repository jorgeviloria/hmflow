//! Escena 3D del rodete: piezas con nombre, en milímetros, que el visor solo
//! dibuja.
//!
//! El eje de giro es z, con el sentido de z del meridiano (la aspiración en
//! −z). Cubo y tapa son superficies de revolución de sus curvas del meridiano.
//! `scene_from` no devuelve error: una pieza que no sale falta en `pieces` y
//! lleva su motivo en `notices`.

use std::f64::consts::TAU;

use hm_geometry::mesh::{Polyline3, TriangleMesh, grid_surface, revolve, revolve_segments};

use crate::blade_camber::{BladeCamber, CamberPoint};
use crate::blade_setup::BladeShape;
use crate::meridian::{MeridionalModel, Wall};

/// Tolerancia de cuerda de la escena en reposo, en mm.
pub const SCENE_TOLERANCE_FINE_MM: f64 = 0.05;
/// Tolerancia de cuerda mientras se arrastra una manija, en mm.
pub const SCENE_TOLERANCE_DRAG_MM: f64 = 0.5;
/// Lo que se admite de tolerancia: fuera de aquí se acota, para que un valor
/// absurdo no genere millones de triángulos.
pub const SCENE_TOLERANCE_RANGE_MM: (f64, f64) = (0.005, 5.0);

/// Radio por debajo del cual el extremo de un perfil está en el eje y no da
/// círculo.
const AXIS_RADIUS_MM: f64 = 1e-9;

/// Qué pieza es.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceId {
    Hub,
    Shroud,
    /// Superficie media del álabe `index`, de 0 a Z − 1.
    BladeMean {
        index: u32,
    },
}

impl PieceId {
    /// Su nombre en la interfaz.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Hub => "Cubo".to_string(),
            Self::Shroud => "Tapa".to_string(),
            Self::BladeMean { index } => format!("Álabe {}", u64::from(index) + 1),
        }
    }

    /// Clave estable para la huella.
    fn key(self) -> u64 {
        match self {
            Self::Hub => 0,
            Self::Shroud => 1,
            Self::BladeMean { index } => 2 + u64::from(index),
        }
    }
}

/// Una pieza de la escena: su superficie y las curvas de sus bordes.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub id: PieceId,
    pub surface: TriangleMesh,
    pub curves: Vec<Polyline3>,
}

/// Por qué una pieza no sale, o sale distinta de lo pedido.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneNotice {
    pub piece: PieceId,
    pub message: String,
}

/// La escena completa.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub pieces: Vec<Piece>,
    pub notices: Vec<SceneNotice>,
    /// Tolerancia de cuerda con la que se hizo, en mm, ya acotada.
    pub tolerance: f64,
}

impl Scene {
    /// La pieza con ese id.
    #[must_use]
    pub fn piece(&self, id: PieceId) -> Option<&Piece> {
        self.pieces.iter().find(|piece| piece.id == id)
    }

    /// La caja de todas las superficies; `None` si no hay vértices.
    #[must_use]
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        self.pieces
            .iter()
            .filter_map(|piece| piece.surface.bounds())
            .reduce(|(low, high), (piece_low, piece_high)| {
                (
                    std::array::from_fn(|axis| low[axis].min(piece_low[axis])),
                    std::array::from_fn(|axis| high[axis].max(piece_high[axis])),
                )
            })
    }

    /// Triángulos de todas las superficies.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.pieces
            .iter()
            .map(|piece| piece.surface.triangles.len())
            .sum()
    }

    /// FNV-1a sobre ids, bits de los vértices, índices y avisos: cambia si
    /// cambia la geometría y es igual entre dos cálculos iguales.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = OFFSET;
        let mut feed = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(PRIME);
            }
        };
        for piece in &self.pieces {
            feed(&piece.id.key().to_le_bytes());
            let mesh = &piece.surface;
            feed(&(mesh.vertices.len() as u64).to_le_bytes());
            for vertex in &mesh.vertices {
                for coordinate in vertex {
                    feed(&coordinate.to_bits().to_le_bytes());
                }
            }
            feed(&(mesh.triangles.len() as u64).to_le_bytes());
            for triangle in &mesh.triangles {
                for index in triangle {
                    feed(&index.to_le_bytes());
                }
            }
        }
        feed(&(self.notices.len() as u64).to_le_bytes());
        for notice in &self.notices {
            feed(&notice.piece.key().to_le_bytes());
            feed(notice.message.as_bytes());
            feed(&[0]);
        }
        hash
    }
}

/// Pasos «uno de cada `s`» que se prueban a lo largo del álabe, del más
/// grueso al más fino.
const STATION_STEPS: [usize; 7] = [20, 10, 8, 5, 4, 2, 1];
/// Columnas a lo ancho del álabe reglado.
const RULED_COLUMNS: usize = 8;
/// Columnas que se prueban a lo ancho del álabe libre, de menos a más.
const FREE_COLUMNS: [usize; 4] = [8, 16, 32, 64];

/// De dónde sale la superficie media del álabe. La forma 2D llega en una
/// tarea posterior.
#[derive(Debug, Clone, PartialEq)]
pub enum BladeSurfaceSource<'a> {
    /// La línea media 3D por línea de corriente, en m y rad.
    Camber(&'a BladeCamber),
    /// No hay superficie: el motivo va al aviso.
    Unavailable(String),
}

/// La tolerancia acotada a [`SCENE_TOLERANCE_RANGE_MM`]; NaN da la fina.
#[must_use]
pub fn clamp_tolerance(tolerance: f64) -> f64 {
    if tolerance.is_nan() {
        return SCENE_TOLERANCE_FINE_MM;
    }
    let (low, high) = SCENE_TOLERANCE_RANGE_MM;
    tolerance.clamp(low, high)
}

/// La escena del rodete con el meridiano en mm (`[z, r]`).
#[must_use]
pub fn scene_from(
    model: &MeridionalModel,
    blade: BladeSurfaceSource<'_>,
    blade_count: u32,
    tolerance: f64,
) -> Scene {
    let tolerance = clamp_tolerance(tolerance);
    let mut pieces = Vec::new();
    let mut notices = Vec::new();
    for (id, wall) in [(PieceId::Hub, Wall::Hub), (PieceId::Shroud, Wall::Shroud)] {
        match wall_piece(model, id, wall, tolerance) {
            Ok(piece) => pieces.push(piece),
            Err(message) => notices.push(SceneNotice { piece: id, message }),
        }
    }
    let blade_notice = |message| SceneNotice {
        piece: PieceId::BladeMean { index: 0 },
        message,
    };
    match blade {
        BladeSurfaceSource::Camber(camber) => match camber_pieces(camber, blade_count, tolerance) {
            Ok(blades) => pieces.extend(blades),
            Err(message) => notices.push(blade_notice(message)),
        },
        BladeSurfaceSource::Unavailable(message) => notices.push(blade_notice(message)),
    }
    Scene {
        pieces,
        notices,
        tolerance,
    }
}

/// Cubo o tapa: la revolución de su curva y los círculos de sus extremos.
fn wall_piece(
    model: &MeridionalModel,
    id: PieceId,
    wall: Wall,
    tolerance: f64,
) -> Result<Piece, String> {
    let what = match wall {
        Wall::Hub => "el cubo",
        Wall::Shroud => "la tapa",
    };
    let curve = model
        .curve(wall.name())
        .ok_or_else(|| format!("falta la curva «{}» en el meridiano", wall.name()))?;
    let surface = revolve(&curve.points, tolerance)
        .map_err(|error| format!("{what} no se pudo revolucionar: {error}"))?;
    let curves = [curve.points.first(), curve.points.last()]
        .into_iter()
        .flatten()
        .filter_map(|&[z, r]| circle(z, r, tolerance))
        .collect();
    Ok(Piece {
        id,
        surface,
        curves,
    })
}

/// El círculo de radio `r` en el plano `z`, cerrado; `None` en el eje.
fn circle(z: f64, r: f64, tolerance: f64) -> Option<Polyline3> {
    if r.is_nan() || r <= AXIS_RADIUS_MM {
        return None;
    }
    let count = revolve_segments(r, tolerance);
    let points = (0..count)
        .map(|step| {
            let angle = std::f64::consts::TAU * step as f64 / count as f64;
            [r * angle.cos(), r * angle.sin(), z]
        })
        .collect();
    Some(Polyline3 {
        points,
        closed: true,
    })
}

/// Los `blade_count` álabes de una línea media 3D: el álabe 0 con
/// `offset = 0` y los demás, girados `−2π·k/Z`, que es el álabe k + 1 de la
/// planta. Cada uno lleva sus cuatro bordes: ataque, salida, cubo y tapa.
fn camber_pieces(
    camber: &BladeCamber,
    blade_count: u32,
    tolerance: f64,
) -> Result<Vec<Piece>, String> {
    if blade_count == 0 {
        return Err("el rodete no tiene álabes".to_string());
    }
    let lines = &camber.lines;
    if lines.len() < 2 {
        return Err(
            "la superficie del álabe necesita al menos dos líneas de corriente".to_string(),
        );
    }
    let nodes = lines[0].points.len();
    if nodes < 2 || lines.iter().any(|line| line.points.len() != nodes) {
        return Err(
            "las líneas medias no tienen el mismo número de nodos: no se puede tejer la superficie"
                .to_string(),
        );
    }
    if lines.windows(2).any(|pair| pair[1].psi <= pair[0].psi) {
        return Err(
            "las líneas de corriente no van del cubo a la tapa con ψ creciente".to_string(),
        );
    }
    let along: Vec<Vec<[f64; 3]>> = lines
        .iter()
        .map(|line| line.points.iter().map(cartesian).collect())
        .collect();
    let stations = kept_stations(&along, tolerance);
    let (cols, points) = match camber.shape {
        BladeShape::Ruled3D => (RULED_COLUMNS, ruled_grid(&along, &stations)),
        _ => free_grid(camber, &stations, tolerance),
    };
    let surface = grid_surface(stations.len(), cols, &points)
        .map_err(|error| format!("la superficie del álabe no se pudo tejer: {error}"))?;
    let rows = stations.len();
    let row = |r: usize| points[r * cols..(r + 1) * cols].to_vec();
    let column = |c: usize| (0..rows).map(|r| points[r * cols + c]).collect::<Vec<_>>();
    let open = |points| Polyline3 {
        points,
        closed: false,
    };
    let curves = vec![
        open(row(0)),
        open(row(rows - 1)),
        open(column(0)),
        open(column(cols - 1)),
    ];
    Ok((0..blade_count)
        .map(|index| {
            let id = PieceId::BladeMean { index };
            if index == 0 {
                return Piece {
                    id,
                    surface: surface.clone(),
                    curves: curves.clone(),
                };
            }
            let angle = -TAU * f64::from(index) / f64::from(blade_count);
            Piece {
                id,
                surface: surface.rotated_z(angle),
                curves: curves.iter().map(|curve| curve.rotated_z(angle)).collect(),
            }
        })
        .collect())
}

/// Un nodo de la línea media en mm, con `offset = 0`:
/// `x = −r·cos θ`, `y = −r·sen θ`, `z` del meridiano.
fn cartesian(point: &CamberPoint) -> [f64; 3] {
    polar_to_mm(point.z, point.r, point.theta)
}

/// De `(z, r, θ)` en m y rad a `[x, y, z]` en mm, con `offset = 0`.
fn polar_to_mm(z: f64, r: f64, theta: f64) -> [f64; 3] {
    let r = r * 1000.0;
    [-r * theta.cos(), -r * theta.sin(), z * 1000.0]
}

/// Los nodos que se conservan a lo largo: uno de cada `s`, con el mayor `s`
/// de [`STATION_STEPS`] tal que ningún nodo omitido de ninguna línea dista
/// más de `tolerance` de la cuerda entre sus vecinos conservados. El último
/// nodo siempre entra.
fn kept_stations(lines: &[Vec<[f64; 3]>], tolerance: f64) -> Vec<usize> {
    let last = lines[0].len() - 1;
    let with_step = |step: usize| {
        let mut kept: Vec<usize> = (0..=last).step_by(step).collect();
        if kept.last() != Some(&last) {
            kept.push(last);
        }
        kept
    };
    STATION_STEPS
        .iter()
        .map(|&step| with_step(step))
        .find(|kept| {
            lines.iter().all(|line| {
                kept.windows(2).all(|pair| {
                    (pair[0] + 1..pair[1]).all(|i| {
                        distance_to_segment(line[i], line[pair[0]], line[pair[1]]) <= tolerance
                    })
                })
            })
        })
        .unwrap_or_else(|| with_step(1))
}

/// Reglada: en cada estación, la recta entre el nodo del cubo y el de la tapa,
/// interpolada en cartesianas.
fn ruled_grid(lines: &[Vec<[f64; 3]>], stations: &[usize]) -> Vec<[f64; 3]> {
    let (hub, shroud) = (&lines[0], &lines[lines.len() - 1]);
    let mut points = Vec::with_capacity(stations.len() * RULED_COLUMNS);
    for &j in stations {
        for col in 0..RULED_COLUMNS {
            let t = col as f64 / (RULED_COLUMNS - 1) as f64;
            points.push(std::array::from_fn(|axis| {
                hub[j][axis] + t * (shroud[j][axis] - hub[j][axis])
            }));
        }
    }
    points
}

/// Libre: spline cúbica natural de `z`, `r` y `θ` frente a ψ por los nodos de
/// cada estación. Columnas: la menor de [`FREE_COLUMNS`] en la que el punto
/// medio de cada tramo dista de su cuerda menos que `tolerance`. La primera y
/// la última columna son los nodos del cubo y de la tapa, sin interpolar.
fn free_grid(camber: &BladeCamber, stations: &[usize], tolerance: f64) -> (usize, Vec<[f64; 3]>) {
    let lines = &camber.lines;
    let psi: Vec<f64> = lines.iter().map(|line| line.psi).collect();
    let (first, last) = (psi[0], psi[psi.len() - 1]);
    let splines: Vec<[Spline; 3]> = stations
        .iter()
        .map(|&j| {
            let values = |pick: fn(&CamberPoint) -> f64| -> Vec<f64> {
                lines.iter().map(|line| pick(&line.points[j])).collect()
            };
            [
                Spline::natural(&psi, values(|p| p.z)),
                Spline::natural(&psi, values(|p| p.r)),
                Spline::natural(&psi, values(|p| p.theta)),
            ]
        })
        .collect();
    let at = |row: usize, cols: usize, col: f64| -> [f64; 3] {
        let station = stations[row];
        if col <= 0.0 {
            return cartesian(&lines[0].points[station]);
        }
        if col >= (cols - 1) as f64 {
            return cartesian(&lines[lines.len() - 1].points[station]);
        }
        let s = first + (last - first) * col / (cols - 1) as f64;
        let [z, r, theta] = &splines[row];
        polar_to_mm(z.at(s), r.at(s), theta.at(s))
    };
    let fits = |cols: usize| {
        (0..stations.len()).all(|row| {
            (0..cols - 1).all(|col| {
                let middle = at(row, cols, col as f64 + 0.5);
                let (a, b) = (at(row, cols, col as f64), at(row, cols, col as f64 + 1.0));
                distance_to_segment(middle, a, b) < tolerance
            })
        })
    };
    let cols = FREE_COLUMNS
        .iter()
        .copied()
        .find(|&cols| fits(cols))
        .unwrap_or(FREE_COLUMNS[FREE_COLUMNS.len() - 1]);
    let points = (0..stations.len())
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .map(|(row, col)| at(row, cols, col as f64))
        .collect();
    (cols, points)
}

/// Distancia de `point` al segmento `a`–`b`.
fn distance_to_segment(point: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let d: [f64; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
    let v: [f64; 3] = std::array::from_fn(|axis| point[axis] - a[axis]);
    let length2 = d.iter().map(|value| value * value).sum::<f64>();
    let t = if length2 > 0.0 {
        (v.iter().zip(&d).map(|(v, d)| v * d).sum::<f64>() / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (0..3)
        .map(|axis| (v[axis] - t * d[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Spline cúbica natural (segunda derivada nula en los extremos). Con dos
/// nodos es la recta.
struct Spline {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// Segunda derivada en cada nodo.
    second: Vec<f64>,
}

impl Spline {
    /// `xs` estrictamente creciente, del mismo largo que `ys` y con dos o más
    /// nodos.
    fn natural(xs: &[f64], ys: Vec<f64>) -> Self {
        let n = xs.len();
        let mut second = vec![0.0; n];
        if n > 2 {
            // Sistema tridiagonal de los nodos interiores, por Thomas.
            let h: Vec<f64> = xs.windows(2).map(|pair| pair[1] - pair[0]).collect();
            let mut diagonal = vec![0.0; n];
            let mut rhs = vec![0.0; n];
            for i in 1..n - 1 {
                diagonal[i] = 2.0 * (h[i - 1] + h[i]);
                rhs[i] = 6.0 * ((ys[i + 1] - ys[i]) / h[i] - (ys[i] - ys[i - 1]) / h[i - 1]);
            }
            for i in 2..n - 1 {
                let factor = h[i - 1] / diagonal[i - 1];
                diagonal[i] -= factor * h[i - 1];
                rhs[i] -= factor * rhs[i - 1];
            }
            for i in (1..n - 1).rev() {
                let next = if i + 1 < n - 1 {
                    h[i] * second[i + 1]
                } else {
                    0.0
                };
                second[i] = (rhs[i] - next) / diagonal[i];
            }
        }
        Self {
            xs: xs.to_vec(),
            ys,
            second,
        }
    }

    /// El valor en `x`, dentro de `[xs[0], xs[n − 1]]`.
    fn at(&self, x: f64) -> f64 {
        let n = self.xs.len();
        let i = self.xs[1..n - 1]
            .iter()
            .take_while(|&&node| node <= x)
            .count();
        let h = self.xs[i + 1] - self.xs[i];
        let a = (self.xs[i + 1] - x) / h;
        let b = (x - self.xs[i]) / h;
        a * self.ys[i]
            + b * self.ys[i + 1]
            + ((a * a * a - a) * self.second[i] + (b * b * b - b) * self.second[i + 1]) * h * h
                / 6.0
    }
}

#[cfg(test)]
mod tests {
    use super::Spline;

    #[test]
    fn the_natural_spline_goes_through_its_nodes_and_is_straight_with_two() {
        let xs = [0.0, 0.25, 0.5, 1.0];
        let spline = Spline::natural(&xs, vec![1.0, 3.0, 2.0, 5.0]);
        for (x, y) in xs.iter().zip([1.0, 3.0, 2.0, 5.0]) {
            assert!((spline.at(*x) - y).abs() < 1e-12);
        }
        let line = Spline::natural(&[0.0, 2.0], vec![1.0, 5.0]);
        assert!((line.at(0.5) - 2.0).abs() < 1e-12);
        // Un cúbico cualquiera con segunda derivada lineal no lo es en general,
        // pero una recta sí se reproduce con cualquier número de nodos.
        let straight = Spline::natural(&xs, xs.iter().map(|x| 2.0 * x + 1.0).collect());
        assert!((straight.at(0.7) - 2.4).abs() < 1e-12);
    }
}
