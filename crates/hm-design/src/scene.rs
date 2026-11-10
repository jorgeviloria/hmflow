//! Escena 3D del rodete: piezas con nombre, en milímetros, que el visor solo
//! dibuja.
//!
//! El eje de giro es z, con el sentido de z del meridiano (la aspiración en
//! −z). Cubo y tapa son superficies de revolución de sus curvas del meridiano.
//! `scene_from` no devuelve error: una pieza que no sale falta en `pieces` y
//! lleva su motivo en `notices`.

use hm_geometry::mesh::{Polyline3, TriangleMesh, revolve, revolve_segments};

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

/// De dónde sale la superficie media del álabe. Las formas 3D y 2D llegan
/// con sus datos prestados en tareas posteriores; entonces gana su vida `'a`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BladeSurfaceSource {
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
    blade: BladeSurfaceSource,
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
    // Las Tareas 5 y 6 dibujan los `blade_count` álabes.
    let _ = blade_count;
    match blade {
        BladeSurfaceSource::Unavailable(message) => notices.push(SceneNotice {
            piece: PieceId::BladeMean { index: 0 },
            message,
        }),
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
