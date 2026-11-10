//! Mallas de triángulos en 3D: superficie de revolución y superficie por una rejilla.
//!
//! Este módulo no sabe de bombas ni elige unidades. Cada vértice lleva su normal unitaria, y los
//! triángulos van en sentido antihorario vistos desde el lado al que apuntan las normales.

use std::f64::consts::{PI, TAU};

use crate::GeometryError;

/// Número mínimo de gajos de una superficie de revolución.
pub const REVOLVE_MIN_SEGMENTS: usize = 24;

/// Giro de la tangente del perfil, en grados, a partir del cual la arista se conserva viva.
pub const CREASE_ANGLE_DEG: f64 = 30.0;

/// Por debajo de esta distancia dos puntos son el mismo, y un radio es cero.
const COINCIDENT: f64 = 1e-9;

/// Malla de triángulos con una normal por vértice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriangleMesh {
    pub vertices: Vec<[f64; 3]>,
    /// Unitarias, una por vértice.
    pub normals: Vec<[f64; 3]>,
    /// Índices de `vertices`, en sentido antihorario vistos desde fuera.
    pub triangles: Vec<[u32; 3]>,
}

impl TriangleMesh {
    /// La misma malla girada `angle` radianes alrededor del eje z.
    #[must_use]
    pub fn rotated_z(&self, angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self {
            vertices: self
                .vertices
                .iter()
                .map(|vertex| rotate_z(*vertex, sin, cos))
                .collect(),
            normals: self
                .normals
                .iter()
                .map(|normal| rotate_z(*normal, sin, cos))
                .collect(),
            triangles: self.triangles.clone(),
        }
    }

    /// Esquinas mínima y máxima de la caja que encierra los vértices. `None` si no hay ninguno.
    #[must_use]
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut vertices = self.vertices.iter();
        let first = *vertices.next()?;
        Some(
            vertices.fold((first, first), |(mut low, mut high), vertex| {
                for ((low, high), value) in low.iter_mut().zip(&mut high).zip(vertex) {
                    *low = low.min(*value);
                    *high = high.max(*value);
                }
                (low, high)
            }),
        )
    }
}

/// Línea quebrada en 3D.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polyline3 {
    pub points: Vec<[f64; 3]>,
    /// El último punto se une con el primero.
    pub closed: bool,
}

impl Polyline3 {
    /// La misma línea girada `angle` radianes alrededor del eje z.
    #[must_use]
    pub fn rotated_z(&self, angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self {
            points: self
                .points
                .iter()
                .map(|point| rotate_z(*point, sin, cos))
                .collect(),
            closed: self.closed,
        }
    }
}

fn rotate_z(point: [f64; 3], sin: f64, cos: f64) -> [f64; 3] {
    [
        point[0] * cos - point[1] * sin,
        point[0] * sin + point[1] * cos,
        point[2],
    ]
}

/// Gajos para que la flecha de la cuerda en el radio `r_max` no pase de `tolerance`:
/// `⌈π / acos(1 − tolerance / r_max)⌉`, con un mínimo de [`REVOLVE_MIN_SEGMENTS`].
///
/// Con un radio o una tolerancia que no valen devuelve el mínimo.
#[must_use]
pub fn revolve_segments(r_max: f64, tolerance: f64) -> usize {
    let ratio = tolerance / r_max;
    if !ratio.is_finite() || ratio <= 0.0 {
        return REVOLVE_MIN_SEGMENTS;
    }
    let half_step = (1.0 - ratio).max(-1.0).acos();
    // `as` satura: si la tolerancia es tan fina que el paso sale nulo, da `usize::MAX`.
    let segments = (PI / half_step).ceil() as usize;
    segments.max(REVOLVE_MIN_SEGMENTS)
}

/// Punto del perfil con la normal que le toca en el plano meridional, `(nz, nr)`.
struct Station {
    z: f64,
    r: f64,
    normal: [f64; 2],
}

impl Station {
    fn on_axis(&self) -> bool {
        on_axis([self.z, self.r])
    }
}

fn on_axis(point: [f64; 2]) -> bool {
    point[1] < COINCIDENT
}

/// Dirección unitaria de `from` a `to` en el plano `(z, r)`. Los puntos no coinciden.
fn direction(from: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    let delta = [to[0] - from[0], to[1] - from[1]];
    let length = delta[0].hypot(delta[1]);
    [delta[0] / length, delta[1] / length]
}

/// La tangente `(tz, tr)` girada a la izquierda: `(nz, nr) = (−tr, tz)`.
fn left_normal(tangent: [f64; 2]) -> [f64; 2] {
    [-tangent[1], tangent[0]]
}

/// Reparte el perfil en estaciones y en bandas (pares de estaciones que se unen con triángulos).
///
/// Dos tramos seguidos comparten estación si la tangente gira poco; si gira más de
/// [`CREASE_ANGLE_DEG`], o si el punto está en el eje, cada tramo tiene la suya con su normal.
/// Un tramo con los dos extremos en el eje no da banda.
fn stations_and_bands(points: &[[f64; 2]]) -> (Vec<Station>, Vec<(usize, usize)>) {
    let crease_cos = CREASE_ANGLE_DEG.to_radians().cos();
    let mut stations: Vec<Station> = Vec::with_capacity(points.len());
    let mut bands = Vec::with_capacity(points.len() - 1);
    let mut shared = None;
    for index in 0..points.len() - 1 {
        let (start, end) = (points[index], points[index + 1]);
        if on_axis(start) && on_axis(end) {
            shared = None;
            continue;
        }
        let tangent = direction(start, end);
        let first = shared.unwrap_or_else(|| {
            stations.push(Station {
                z: start[0],
                r: start[1],
                normal: left_normal(tangent),
            });
            stations.len() - 1
        });
        let smooth_tangent = points
            .get(index + 2)
            .filter(|_| !on_axis(end))
            .and_then(|next| {
                let ahead = direction(end, *next);
                let turn_cos = tangent[0] * ahead[0] + tangent[1] * ahead[1];
                // Con poco giro, la derivada centrada del perfil es la tangente en el punto.
                (turn_cos >= crease_cos).then(|| direction(start, *next))
            });
        stations.push(Station {
            z: end[0],
            r: end[1],
            normal: left_normal(smooth_tangent.unwrap_or(tangent)),
        });
        let last = stations.len() - 1;
        bands.push((first, last));
        shared = smooth_tangent.map(|_| last);
    }
    (stations, bands)
}

/// Superficie de revolución de un perfil alrededor del eje z.
///
/// `profile` son puntos `[z, r]`; cada uno da los vértices `(r·cos a, r·sin a, z)`. El número de
/// gajos sale de [`revolve_segments`] con el radio mayor del perfil.
///
/// La normal es la tangente del perfil girada a la izquierda en el plano `(z, r)` y llevada al
/// ángulo `a`: con el perfil recorrido hacia +z apunta hacia r creciente. Los puntos seguidos
/// repetidos se descartan, un punto en el eje da un solo vértice con su abanico, y donde la
/// tangente gira más de [`CREASE_ANGLE_DEG`] el punto se duplica para conservar la arista.
///
/// # Errors
///
/// - [`GeometryError::TooFewSamples`] con menos de dos puntos.
/// - [`GeometryError::BadTolerance`] si la tolerancia no es un número mayor que cero, o es tan
///   fina que los vértices no caben en índices de 32 bits.
/// - [`GeometryError::NegativeRadius`] si algún radio es negativo.
/// - [`GeometryError::Degenerate`] si hay coordenadas no finitas, o si no queda superficie: un
///   solo punto repetido o todo el perfil en el eje.
pub fn revolve(profile: &[[f64; 2]], tolerance: f64) -> Result<TriangleMesh, GeometryError> {
    if profile.len() < 2 {
        return Err(GeometryError::TooFewSamples);
    }
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(GeometryError::BadTolerance);
    }
    if profile.iter().flatten().any(|value| !value.is_finite()) {
        return Err(GeometryError::Degenerate);
    }
    if profile.iter().any(|point| point[1] < 0.0) {
        return Err(GeometryError::NegativeRadius);
    }

    let mut points: Vec<[f64; 2]> = Vec::with_capacity(profile.len());
    for &point in profile {
        let repeated = points
            .last()
            .is_some_and(|last| (point[0] - last[0]).hypot(point[1] - last[1]) < COINCIDENT);
        if !repeated {
            points.push(point);
        }
    }
    let r_max = points.iter().map(|point| point[1]).fold(0.0, f64::max);
    if points.len() < 2 || r_max < COINCIDENT {
        return Err(GeometryError::Degenerate);
    }

    let segments = revolve_segments(r_max, tolerance);
    let (stations, bands) = stations_and_bands(&points);
    let fits = stations
        .len()
        .checked_mul(segments)
        .is_some_and(|count| u32::try_from(count).is_ok());
    if !fits {
        return Err(GeometryError::BadTolerance);
    }

    let turns: Vec<(f64, f64)> = (0..segments)
        .map(|step| (TAU * step as f64 / segments as f64).sin_cos())
        .collect();
    let mut mesh = TriangleMesh::default();
    let mut firsts = Vec::with_capacity(stations.len());
    for station in &stations {
        firsts.push(mesh.vertices.len() as u32);
        let [nz, nr] = station.normal;
        if station.on_axis() {
            // En el eje la normal solo puede ser axial: se queda con el lado al que mira la banda.
            mesh.vertices.push([0.0, 0.0, station.z]);
            mesh.normals.push([0.0, 0.0, nz.signum()]);
        } else {
            for (sin, cos) in &turns {
                mesh.vertices
                    .push([station.r * cos, station.r * sin, station.z]);
                mesh.normals.push([nr * cos, nr * sin, nz]);
            }
        }
    }

    let count = segments as u32;
    for (start, end) in bands {
        let (low, high) = (firsts[start], firsts[end]);
        for step in 0..count {
            let next = (step + 1) % count;
            match (stations[start].on_axis(), stations[end].on_axis()) {
                (false, false) => {
                    mesh.triangles.push([low + step, low + next, high + next]);
                    mesh.triangles.push([low + step, high + next, high + step]);
                }
                (true, false) => mesh.triangles.push([low, high + next, high + step]),
                (false, true) => mesh.triangles.push([low + step, low + next, high]),
                // Un tramo con los dos extremos en el eje no llega a ser banda.
                (true, true) => {}
            }
        }
    }
    Ok(mesh)
}

fn sub(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

/// Superficie por una rejilla ordenada de `rows × cols` puntos, con `points[row * cols + col]`.
///
/// La normal de cada vértice sale de las diferencias en la rejilla, centradas dentro y laterales
/// en los bordes: es el producto de la diferencia a lo largo de las columnas por la diferencia a
/// lo largo de las filas.
///
/// # Errors
///
/// - [`GeometryError::TooFewSamples`] con menos de dos filas o de dos columnas.
/// - [`GeometryError::GridMismatch`] si `rows · cols` no es el número de puntos.
/// - [`GeometryError::Degenerate`] si hay coordenadas no finitas o si en algún vértice no hay
///   normal, porque la rejilla ahí no tiene área.
pub fn grid_surface(
    rows: usize,
    cols: usize,
    points: &[[f64; 3]],
) -> Result<TriangleMesh, GeometryError> {
    if rows < 2 || cols < 2 {
        return Err(GeometryError::TooFewSamples);
    }
    if rows.checked_mul(cols) != Some(points.len()) || u32::try_from(points.len()).is_err() {
        return Err(GeometryError::GridMismatch);
    }
    if points.iter().flatten().any(|value| !value.is_finite()) {
        return Err(GeometryError::Degenerate);
    }

    let at = |row: usize, col: usize| points[row * cols + col];
    let mut normals = Vec::with_capacity(points.len());
    for row in 0..rows {
        for col in 0..cols {
            let across = sub(
                at(row, (col + 1).min(cols - 1)),
                at(row, col.saturating_sub(1)),
            );
            let along = sub(
                at((row + 1).min(rows - 1), col),
                at(row.saturating_sub(1), col),
            );
            let normal = cross(across, along);
            let length =
                (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            if length < COINCIDENT * COINCIDENT {
                return Err(GeometryError::Degenerate);
            }
            normals.push([normal[0] / length, normal[1] / length, normal[2] / length]);
        }
    }

    let index = |row: usize, col: usize| (row * cols + col) as u32;
    let mut triangles = Vec::with_capacity(2 * (rows - 1) * (cols - 1));
    for row in 0..rows - 1 {
        for col in 0..cols - 1 {
            let corners = [
                index(row, col),
                index(row, col + 1),
                index(row + 1, col + 1),
            ];
            triangles.push(corners);
            triangles.push([corners[0], corners[2], index(row + 1, col)]);
        }
    }
    Ok(TriangleMesh {
        vertices: points.to_vec(),
        normals,
        triangles,
    })
}
