//! Escena 3D del rodete: piezas con nombre, en milímetros, que el visor solo
//! dibuja.
//!
//! El eje de giro es z, con el sentido de z del meridiano (la aspiración en
//! −z). Cubo y tapa son superficies de revolución de sus curvas del meridiano.
//! `scene_from` no devuelve error: una pieza que no sale falta en `pieces` y
//! lleva su motivo en `notices`.

use std::f64::consts::TAU;
use std::ops::Range;

use hm_geometry::mesh::{Polyline3, TriangleMesh, grid_surface, revolve, revolve_segments};

use crate::blade_camber::{BladeCamber, CamberPoint};
use crate::blade_setup::BladeShape;
use crate::blade_solid::{BladeSolid, SolidLine, SolidPoint};
use crate::geometry::BladePoint;
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
/// Un tramo de pared con |Δr| menor que esto es axial: no da un z por radio.
const AXIAL_RUN_MM: f64 = 1e-9;

/// Qué pieza es.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PieceId {
    Hub,
    Shroud,
    /// Superficie media del álabe `index`, de 0 a Z − 1.
    BladeMean {
        index: u32,
    },
    /// El álabe `index` con espesor: una pieza cerrada de seis caras.
    Blade {
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
            Self::BladeMean { index } => format!("Superficie media {}", u64::from(index) + 1),
            Self::Blade { index } => format!("Álabe {}", u64::from(index) + 1),
        }
    }

    /// Clave estable para la huella.
    fn key(self) -> u64 {
        match self {
            Self::Hub => 0,
            Self::Shroud => 1,
            Self::BladeMean { index } => 2 + u64::from(index),
            // Otro espacio de claves: la huella de una escena sin álabes con
            // espesor no cambia.
            Self::Blade { index } => (1 << 32) + u64::from(index),
        }
    }
}

/// Las seis caras de un álabe con espesor, en el orden en que se tejen y se
/// unen en la pieza.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BladeFace {
    Pressure,
    Suction,
    HubBand,
    ShroudBand,
    LeadingEdge,
    TrailingEdge,
}

impl BladeFace {
    /// Las seis, en el orden de la pieza.
    pub const ALL: [Self; 6] = [
        Self::Pressure,
        Self::Suction,
        Self::HubBand,
        Self::ShroudBand,
        Self::LeadingEdge,
        Self::TrailingEdge,
    ];

    /// Su nombre en la interfaz.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Pressure => "Cara de presión",
            Self::Suction => "Cara de succión",
            Self::HubBand => "Banda del cubo",
            Self::ShroudBand => "Banda de la tapa",
            Self::LeadingEdge => "Canto de entrada",
            Self::TrailingEdge => "Canto de salida",
        }
    }
}

/// Una pieza de la escena: su superficie y las curvas de sus bordes.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub id: PieceId,
    pub surface: TriangleMesh,
    pub curves: Vec<Polyline3>,
    /// Los triángulos de `surface` de cada cara, en el orden de
    /// [`BladeFace::ALL`]; vacío en las piezas que no son un álabe con
    /// espesor.
    pub faces: Vec<(BladeFace, Range<u32>)>,
}

impl Piece {
    /// Una cara del álabe como malla propia, con solo sus vértices; `None` si
    /// la pieza no la tiene.
    #[must_use]
    pub fn face(&self, face: BladeFace) -> Option<TriangleMesh> {
        let (_, range) = self.faces.iter().find(|(named, _)| *named == face)?;
        let triangles = &self.surface.triangles[range.start as usize..range.end as usize];
        // Los vértices de la cara, en el orden de la pieza (el de su rejilla).
        let mut used: Vec<u32> = triangles.iter().flatten().copied().collect();
        used.sort_unstable();
        used.dedup();
        let remap: std::collections::HashMap<u32, u32> = used
            .iter()
            .enumerate()
            .map(|(new, old)| (*old, new as u32))
            .collect();
        let mut mesh = TriangleMesh {
            vertices: used
                .iter()
                .map(|&index| self.surface.vertices[index as usize])
                .collect(),
            normals: used
                .iter()
                .map(|&index| self.surface.normals[index as usize])
                .collect(),
            triangles: Vec::with_capacity(triangles.len()),
        };
        mesh.triangles = triangles
            .iter()
            .map(|triangle| triangle.map(|index| remap[&index]))
            .collect();
        Some(mesh)
    }
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

/// De dónde sale la superficie media del álabe.
#[derive(Debug, Clone, PartialEq)]
pub enum BladeSurfaceSource<'a> {
    /// La línea media 3D por línea de corriente, en m y rad.
    Camber(&'a BladeCamber),
    /// La línea media de una forma 2D (`r`, `θ` en m y rad), tendida en z
    /// entre cubo y tapa: un álabe cilíndrico.
    Planar(&'a [BladePoint]),
    /// El álabe con espesor: la línea media (para la superficie media, que se
    /// conserva) y el sólido, o el motivo de que no salga. Con `Err`, solo
    /// las superficies medias y un aviso.
    Solid {
        camber: &'a BladeCamber,
        solid: Result<&'a BladeSolid, String>,
    },
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
            Ok((blades, missed)) => {
                pieces.extend(blades);
                notices.extend(missed.map(blade_notice));
            }
            Err(message) => notices.push(blade_notice(message)),
        },
        BladeSurfaceSource::Planar(blade) => {
            match planar_pieces(model, blade, blade_count, tolerance) {
                Ok(blades) => pieces.extend(blades),
                Err(message) => notices.push(blade_notice(message)),
            }
        }
        BladeSurfaceSource::Solid { camber, solid } => {
            match camber_pieces(camber, blade_count, tolerance) {
                Ok((blades, missed)) => {
                    pieces.extend(blades);
                    notices.extend(missed.map(blade_notice));
                }
                Err(message) => notices.push(blade_notice(message)),
            }
            let thick_notice = |message| SceneNotice {
                piece: PieceId::Blade { index: 0 },
                message,
            };
            let fallback = |reason: &str| {
                thick_notice(format!(
                    "el álabe con espesor no sale ({reason}); se dibuja la superficie media"
                ))
            };
            match solid {
                Ok(solid) => match thick_pieces(camber, solid, blade_count, tolerance) {
                    Ok((blades, missed)) => {
                        pieces.extend(blades);
                        notices.extend(missed.map(thick_notice));
                    }
                    Err(message) => notices.push(fallback(&message)),
                },
                Err(reason) => notices.push(fallback(&reason)),
            }
        }
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
        faces: Vec::new(),
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
/// Con la forma libre devuelve también el aviso si ni con las columnas
/// máximas se alcanza la tolerancia.
fn camber_pieces(
    camber: &BladeCamber,
    blade_count: u32,
    tolerance: f64,
) -> Result<(Vec<Piece>, Option<String>), String> {
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
    let (cols, points, missed) = match camber.shape {
        BladeShape::Ruled3D => (RULED_COLUMNS, ruled_grid(&along, &stations), None),
        _ => free_grid(camber, &stations, tolerance),
    };
    let notice = missed.map(|deviation| {
        format!(
            "la superficie del álabe no alcanza la tolerancia de {tolerance} mm con {cols} \
             columnas a lo ancho: se aparta hasta {deviation:.3} mm"
        )
    });
    Ok((
        woven_blades(stations.len(), cols, &points, blade_count)?,
        notice,
    ))
}

/// Los `blade_count` álabes de una forma 2D: en cada punto de la línea, el z
/// del cubo y el de la tapa en su radio; dos columnas, cubo y tapa.
fn planar_pieces(
    model: &MeridionalModel,
    blade: &[BladePoint],
    blade_count: u32,
    tolerance: f64,
) -> Result<Vec<Piece>, String> {
    if blade_count == 0 {
        return Err("el rodete no tiene álabes".to_string());
    }
    if blade.len() < 2 {
        return Err("la línea media 2D tiene menos de dos puntos".to_string());
    }
    let wall = |wall: Wall| {
        model
            .curve(wall.name())
            .map(|curve| curve.points.as_slice())
            .ok_or_else(|| format!("falta la curva «{}» en el meridiano", wall.name()))
    };
    let (hub, shroud) = (wall(Wall::Hub)?, wall(Wall::Shroud)?);
    let mut points = Vec::with_capacity(blade.len() * 2);
    for point in blade {
        let r = point.r * 1000.0;
        let z_of = |profile: &[[f64; 2]], what: &str| {
            wall_z_at(profile, r, tolerance).ok_or_else(|| {
                format!(
                    "la forma 2D no se puede tender entre cubo y tapa: {what} no tiene un único z en r = {r:.1} mm"
                )
            })
        };
        let (z_hub, z_shroud) = (z_of(hub, "el cubo")?, z_of(shroud, "la tapa")?);
        let [x, y, _] = polar_to_mm(0.0, point.r, point.theta);
        points.push([x, y, z_hub]);
        points.push([x, y, z_shroud]);
    }
    woven_blades(blade.len(), 2, &points, blade_count)
}

/// El z de la pared `profile` (`[z, r]` en mm) en el radio `r`: interpolado
/// en el único tramo que cruza `r`. `None` si `r` queda fuera, si lo cruzan
/// tramos con z distintos (más de `tolerance`) o si cae en un tramo axial.
fn wall_z_at(profile: &[[f64; 2]], r: f64, tolerance: f64) -> Option<f64> {
    let mut found: Option<f64> = None;
    for pair in profile.windows(2) {
        let ([za, ra], [zb, rb]) = (pair[0], pair[1]);
        let z = if (rb - ra).abs() < AXIAL_RUN_MM {
            if (r - ra).abs() > AXIAL_RUN_MM {
                continue;
            }
            if (zb - za).abs() > tolerance {
                return None;
            }
            za
        } else {
            if r < ra.min(rb) || r > ra.max(rb) {
                continue;
            }
            za + (r - ra) / (rb - ra) * (zb - za)
        };
        match found {
            Some(previous) if (previous - z).abs() > tolerance => return None,
            Some(_) => {}
            None => found = Some(z),
        }
    }
    found
}

/// Teje la rejilla del álabe 0 (`rows` × `cols`, por filas) y la gira para
/// los demás. Bordes: ataque (fila 0), salida (última fila), cubo (columna 0)
/// y tapa (última columna).
fn woven_blades(
    rows: usize,
    cols: usize,
    points: &[[f64; 3]],
    blade_count: u32,
) -> Result<Vec<Piece>, String> {
    let surface = grid_surface(rows, cols, points)
        .map_err(|error| format!("la superficie del álabe no se pudo tejer: {error}"))?;
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
                    faces: Vec::new(),
                };
            }
            let angle = -TAU * f64::from(index) / f64::from(blade_count);
            Piece {
                id,
                surface: surface.rotated_z(angle),
                curves: curves.iter().map(|curve| curve.rotated_z(angle)).collect(),
                faces: Vec::new(),
            }
        })
        .collect())
}

/// Los `blade_count` álabes con espesor: cada uno, una pieza cerrada de seis
/// caras tejidas por separado (presión, succión, banda del cubo, banda de la
/// tapa, canto de entrada y canto de salida) y unidas sin fundirse: ningún
/// triángulo cruza la arista entre una cara y otra, y la etapa 2 puede
/// cambiar el canto de entrada sin rehacer las otras cinco. El álabe 0 sale
/// con `offset = 0` y los demás, girados `−2π·k/Z`.
///
/// Las seis rejillas comparten estaciones y columnas, y sus puntos de borde,
/// bit a bit: la malla resultante es cerrada. Las estaciones son las de
/// [`kept_stations`] sobre las dos caras a la vez; las columnas a lo ancho,
/// las de la superficie media (8 en la reglada, rectas entre cubo y tapa de
/// cada cara; en la libre, spline en ψ con las columnas que cumplan la
/// tolerancia en las dos caras). Cada rejilla se orienta para que su normal
/// mire hacia fuera.
fn thick_pieces(
    camber: &BladeCamber,
    solid: &BladeSolid,
    blade_count: u32,
    tolerance: f64,
) -> Result<(Vec<Piece>, Option<String>), String> {
    if blade_count == 0 {
        return Err("el rodete no tiene álabes".to_string());
    }
    let lines = &solid.lines;
    let nodes = lines.first().map_or(0, |line| line.pressure.len());
    if lines.len() < 2
        || lines.len() != camber.lines.len()
        || nodes < 2
        || lines
            .iter()
            .any(|line| [line.pressure.len(), line.middle.len(), line.suction.len()] != [nodes; 3])
    {
        return Err("las caras del álabe no tienen las mismas estaciones".to_string());
    }
    if lines.windows(2).any(|pair| pair[1].psi <= pair[0].psi) {
        return Err(
            "las líneas de corriente no van del cubo a la tapa con ψ creciente".to_string(),
        );
    }

    // [z, r, θ] en m y rad de cada cara, por línea de corriente.
    let polar = |pick: fn(&SolidLine) -> &Vec<SolidPoint>| -> Vec<Vec<[f64; 3]>> {
        lines
            .iter()
            .map(|line| pick(line).iter().map(|p| [p.z, p.r, p.theta]).collect())
            .collect()
    };
    let faces = [
        polar(|line| &line.pressure),
        polar(|line| &line.middle),
        polar(|line| &line.suction),
    ];
    let millimetres: Vec<Vec<Vec<[f64; 3]>>> = faces
        .iter()
        .map(|face| {
            face.iter()
                .map(|line| line.iter().map(|p| polar_to_mm(p[0], p[1], p[2])).collect())
                .collect()
        })
        .collect();
    // Las estaciones valen para las dos caras laterales a la vez.
    let both: Vec<Vec<[f64; 3]>> = millimetres[0]
        .iter()
        .chain(&millimetres[2])
        .cloned()
        .collect();
    let stations = kept_stations(&both, tolerance);
    let (cols, grids, missed) = match camber.shape {
        BladeShape::Ruled3D => (
            RULED_COLUMNS,
            millimetres
                .iter()
                .map(|face| ruled_exact(face, &stations))
                .collect::<Vec<_>>(),
            None,
        ),
        _ => {
            let psi: Vec<f64> = lines.iter().map(|line| line.psi).collect();
            free_columns(&psi, &faces, &[0, 2], &stations, tolerance)
        }
    };
    let notice = missed.map(|deviation| {
        format!(
            "la superficie del álabe con espesor no alcanza la tolerancia de {tolerance} mm con \
             {cols} columnas a lo ancho: se aparta hasta {deviation:.3} mm"
        )
    });
    let rows = stations.len();
    let (pressure, middle, suction) = (&grids[0], &grids[1], &grids[2]);

    // Una rejilla de `rows × cols` puntos escogidos por `pick(fila, columna)`.
    let weave = |rows: usize, cols: usize, pick: &dyn Fn(usize, usize) -> [f64; 3]| {
        let points: Vec<[f64; 3]> = (0..rows)
            .flat_map(|row| (0..cols).map(move |col| (row, col)))
            .map(|(row, col)| pick(row, col))
            .collect();
        grid_surface(rows, cols, &points)
            .map_err(|error| format!("el álabe con espesor no se pudo tejer: {error}"))
    };
    let side = |grid: &[[f64; 3]]| weave(rows, cols, &|row, col| grid[row * cols + col]);
    let band = |col: usize| {
        weave(rows, 3, &|row, k| {
            [pressure, middle, suction][k][row * cols + col]
        })
    };
    let edge = |row: usize| {
        weave(3, cols, &|k, col| {
            [pressure, middle, suction][k][row * cols + col]
        })
    };
    let meshes = [
        side(pressure)?,
        side(suction)?,
        band(0)?,
        band(cols - 1)?,
        edge(0)?,
        edge(rows - 1)?,
    ];

    // Hacia fuera, con una referencia local de cada cara (no «desde el centro
    // del álabe», que un álabe muy enrollado desmiente): la presión mira de la
    // succión a la presión y al revés; las bandas, del interior de la
    // superficie media hacia el cubo y la tapa; los cantos, de la estación
    // vecina hacia la primera y la última. Todas se emparejan por estación.
    let mean = |points: &mut dyn Iterator<Item = [f64; 3]>| -> [f64; 3] {
        let (sum, count) = points.fold(([0.0; 3], 0.0), |(sum, count), p| {
            (std::array::from_fn(|axis| sum[axis] + p[axis]), count + 1.0)
        });
        sum.map(|value| value / count)
    };
    let minus =
        |a: [f64; 3], b: [f64; 3]| -> [f64; 3] { std::array::from_fn(|axis| a[axis] - b[axis]) };
    let column =
        |grid: &[[f64; 3]], col: usize| mean(&mut (0..rows).map(|row| grid[row * cols + col]));
    let row_of =
        |grid: &[[f64; 3]], row: usize| mean(&mut (0..cols).map(|col| grid[row * cols + col]));
    let toward_pressure = minus(
        mean(&mut pressure.iter().copied()),
        mean(&mut suction.iter().copied()),
    );
    let references = [
        toward_pressure,
        toward_pressure.map(|value| -value),
        minus(column(middle, 0), column(middle, 1)),
        minus(column(middle, cols - 1), column(middle, cols - 2)),
        minus(row_of(middle, 0), row_of(middle, 1)),
        minus(row_of(middle, rows - 1), row_of(middle, rows - 2)),
    ];
    let meshes: Vec<TriangleMesh> = meshes
        .into_iter()
        .zip(references)
        .map(|(mesh, reference)| outwards(mesh, reference))
        .collect();

    let mut faces_ranges = Vec::with_capacity(6);
    let mut start = 0u32;
    for (face, mesh) in BladeFace::ALL.into_iter().zip(&meshes) {
        let end = start + mesh.triangles.len() as u32;
        faces_ranges.push((face, start..end));
        start = end;
    }
    let surface = TriangleMesh::merged(&meshes);

    // Las 12 aristas: cuatro a lo largo (presión–cubo, presión–tapa,
    // succión–cubo, succión–tapa), cuatro del canto de entrada y cuatro del
    // de salida (presión, succión, cubo y tapa).
    let open = |points: Vec<[f64; 3]>| Polyline3 {
        points,
        closed: false,
    };
    let along = |grid: &[[f64; 3]], col: usize| -> Vec<[f64; 3]> {
        (0..rows).map(|row| grid[row * cols + col]).collect()
    };
    let across = |grid: &[[f64; 3]], row: usize| grid[row * cols..(row + 1) * cols].to_vec();
    let thickness = |row: usize, col: usize| -> Vec<[f64; 3]> {
        vec![
            pressure[row * cols + col],
            middle[row * cols + col],
            suction[row * cols + col],
        ]
    };
    let mut curves = vec![
        open(along(pressure, 0)),
        open(along(pressure, cols - 1)),
        open(along(suction, 0)),
        open(along(suction, cols - 1)),
    ];
    for row in [0, rows - 1] {
        curves.extend([
            open(across(pressure, row)),
            open(across(suction, row)),
            open(thickness(row, 0)),
            open(thickness(row, cols - 1)),
        ]);
    }

    let blades = (0..blade_count)
        .map(|index| {
            let id = PieceId::Blade { index };
            if index == 0 {
                return Piece {
                    id,
                    surface: surface.clone(),
                    curves: curves.clone(),
                    faces: faces_ranges.clone(),
                };
            }
            let angle = -TAU * f64::from(index) / f64::from(blade_count);
            Piece {
                id,
                surface: surface.rotated_z(angle),
                curves: curves.iter().map(|curve| curve.rotated_z(angle)).collect(),
                faces: faces_ranges.clone(),
            }
        })
        .collect();
    Ok((blades, notice))
}

/// La malla con la normal del lado de `reference`: si la suma de las normales
/// de sus triángulos mira al contrario, se invierte el orden de cada triángulo
/// y se cambia el signo de las normales.
fn outwards(mut mesh: TriangleMesh, reference: [f64; 3]) -> TriangleMesh {
    let mut normal = [0.0; 3];
    for triangle in &mesh.triangles {
        let [a, b, c] = triangle.map(|index| mesh.vertices[index as usize]);
        let (u, v): ([f64; 3], [f64; 3]) = (
            std::array::from_fn(|axis| b[axis] - a[axis]),
            std::array::from_fn(|axis| c[axis] - a[axis]),
        );
        normal[0] += u[1] * v[2] - u[2] * v[1];
        normal[1] += u[2] * v[0] - u[0] * v[2];
        normal[2] += u[0] * v[1] - u[1] * v[0];
    }
    let towards: f64 = (0..3).map(|axis| normal[axis] * reference[axis]).sum();
    if towards < 0.0 {
        for triangle in &mut mesh.triangles {
            triangle.swap(1, 2);
        }
        for normal in &mut mesh.normals {
            *normal = normal.map(|value| -value);
        }
    }
    mesh
}

/// Reglada: en cada estación, la recta entre el nodo del cubo y el de la tapa,
/// con los dos extremos exactos (no `a + 1·(b − a)`), para que las caras
/// compartan sus puntos de borde bit a bit.
fn ruled_exact(lines: &[Vec<[f64; 3]>], stations: &[usize]) -> Vec<[f64; 3]> {
    let (hub, shroud) = (&lines[0], &lines[lines.len() - 1]);
    let mut points = Vec::with_capacity(stations.len() * RULED_COLUMNS);
    for &j in stations {
        for col in 0..RULED_COLUMNS {
            points.push(if col == 0 {
                hub[j]
            } else if col == RULED_COLUMNS - 1 {
                shroud[j]
            } else {
                let t = col as f64 / (RULED_COLUMNS - 1) as f64;
                std::array::from_fn(|axis| hub[j][axis] + t * (shroud[j][axis] - hub[j][axis]))
            });
        }
    }
    points
}

/// Libre, para varias caras a la vez: spline cúbica natural de `z`, `r` y `θ`
/// frente a ψ por los nodos de cada estación, con las mismas columnas en
/// todas. Columnas: la menor de [`FREE_COLUMNS`] en la que el punto medio de
/// cada tramo dista de su cuerda menos que `tolerance` en las caras
/// `checked`. La primera y la última columna son los nodos del cubo y de la
/// tapa, sin interpolar. `faces[f][línea][nodo]` es `[z, r, θ]` en m y rad.
fn free_columns(
    psi: &[f64],
    faces: &[Vec<Vec<[f64; 3]>>],
    checked: &[usize],
    stations: &[usize],
    tolerance: f64,
) -> (usize, Vec<Vec<[f64; 3]>>, Option<f64>) {
    let (first, last) = (psi[0], psi[psi.len() - 1]);
    let splines: Vec<Vec<[Spline; 3]>> = faces
        .iter()
        .map(|lines| {
            stations
                .iter()
                .map(|&j| {
                    let values = |axis: usize| -> Vec<f64> {
                        lines.iter().map(|line| line[j][axis]).collect()
                    };
                    [
                        Spline::natural(psi, values(0)),
                        Spline::natural(psi, values(1)),
                        Spline::natural(psi, values(2)),
                    ]
                })
                .collect()
        })
        .collect();
    let at = |face: usize, row: usize, cols: usize, col: f64| -> [f64; 3] {
        let station = stations[row];
        let lines = &faces[face];
        if col <= 0.0 {
            let p = lines[0][station];
            return polar_to_mm(p[0], p[1], p[2]);
        }
        if col >= (cols - 1) as f64 {
            let p = lines[lines.len() - 1][station];
            return polar_to_mm(p[0], p[1], p[2]);
        }
        let s = first + (last - first) * col / (cols - 1) as f64;
        let [z, r, theta] = &splines[face][row];
        polar_to_mm(z.at(s), r.at(s), theta.at(s))
    };
    let deviation = |cols: usize| {
        checked
            .iter()
            .flat_map(|&face| {
                (0..stations.len())
                    .flat_map(|row| (0..cols - 1).map(move |col| (row, col)))
                    .map(move |(row, col)| (face, row, col))
            })
            .map(|(face, row, col)| {
                let middle = at(face, row, cols, col as f64 + 0.5);
                let (a, b) = (
                    at(face, row, cols, col as f64),
                    at(face, row, cols, col as f64 + 1.0),
                );
                distance_to_segment(middle, a, b)
            })
            .fold(0.0, f64::max)
    };
    let most = FREE_COLUMNS[FREE_COLUMNS.len() - 1];
    let (cols, missed) = match FREE_COLUMNS
        .iter()
        .copied()
        .find(|&cols| deviation(cols) < tolerance)
    {
        Some(cols) => (cols, None),
        None => (most, Some(deviation(most))),
    };
    let grids = (0..faces.len())
        .map(|face| {
            (0..stations.len())
                .flat_map(|row| (0..cols).map(move |col| (row, col)))
                .map(|(row, col)| at(face, row, cols, col as f64))
                .collect()
        })
        .collect();
    (cols, grids, missed)
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
fn free_grid(
    camber: &BladeCamber,
    stations: &[usize],
    tolerance: f64,
) -> (usize, Vec<[f64; 3]>, Option<f64>) {
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
    // El mayor desvío entre la cuerda y el punto medio de cada tramo.
    let deviation = |cols: usize| {
        (0..stations.len())
            .flat_map(|row| (0..cols - 1).map(move |col| (row, col)))
            .map(|(row, col)| {
                let middle = at(row, cols, col as f64 + 0.5);
                let (a, b) = (at(row, cols, col as f64), at(row, cols, col as f64 + 1.0));
                distance_to_segment(middle, a, b)
            })
            .fold(0.0, f64::max)
    };
    let most = FREE_COLUMNS[FREE_COLUMNS.len() - 1];
    let (cols, missed) = match FREE_COLUMNS
        .iter()
        .copied()
        .find(|&cols| deviation(cols) < tolerance)
    {
        Some(cols) => (cols, None),
        None => (most, Some(deviation(most))),
    };
    let points = (0..stations.len())
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .map(|(row, col)| at(row, cols, col as f64))
        .collect();
    (cols, points, missed)
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
    use super::{Spline, wall_z_at};

    const TOL: f64 = 0.05;

    #[test]
    fn wall_z_is_interpolated_on_a_monotone_wall() {
        assert_eq!(
            wall_z_at(&[[0.0, 10.0], [10.0, 20.0]], 15.0, TOL),
            Some(5.0)
        );
    }

    #[test]
    fn an_axial_run_has_no_single_z() {
        let wall = [[0.0, 10.0], [5.0, 10.0], [10.0, 20.0]];
        assert_eq!(wall_z_at(&wall, 10.0, TOL), None);
    }

    #[test]
    fn a_radius_outside_the_wall_has_no_z() {
        assert_eq!(wall_z_at(&[[0.0, 10.0], [10.0, 20.0]], 25.0, TOL), None);
    }

    #[test]
    fn a_wall_that_folds_back_has_no_single_z() {
        let wall = [[0.0, 10.0], [10.0, 20.0], [20.0, 15.0], [30.0, 25.0]];
        assert_eq!(wall_z_at(&wall, 17.0, TOL), None);
    }

    #[test]
    fn a_shared_vertex_gives_one_z() {
        let wall = [[0.0, 10.0], [10.0, 20.0], [15.0, 30.0]];
        assert_eq!(wall_z_at(&wall, 20.0, TOL), Some(10.0));
    }

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
