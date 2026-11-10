//! Líneas de corriente del corte meridional.
//!
//! El canal se reparte en N − 1 canales parciales de igual caudal. Con cm
//! constante a lo largo de cada estación, igual caudal es igual ∫r dl entre
//! líneas vecinas: ΔQ = 2π·r·Δb·cm (Gülich, *Centrifugal Pumps*, 4.ª ed., 2020,
//! §7.2.1, método de Kaplan, paso 1, p. 419, «frecuentemente con la hipótesis
//! cm = constante»; Srinivasan, *Rotodynamic Pumps*, 2.ª ed., ec. 4.63,
//! r·Δb = cte).
//!
//! Las estaciones son cuasi-normales ([`quasi_normals`]): rectas del cubo a
//! la tapa que forman el mismo ángulo con las dos paredes, como la cuerda entre
//! los puntos de contacto de un círculo inscrito. Son las «normales estimadas»
//! de Gülich (p. 419) y quedan perpendiculares a la dirección media del flujo,
//! así que 2π·r·b es la sección de paso de la estación. Van del plano de
//! entrada a la salida, porque el borde de ataque puede meterse en el ojo y
//! las líneas tienen que llegar a él.
//!
//! Emparejar las dos tapas por igual fracción de su longitud, como hace
//! [`MeridionalModel::channel_from`], no sirve aquí: en la curva del canal esas
//! rectas se apartan decenas de grados de la normal y su 2π·r·b ya no es una
//! sección de paso.
//!
//! Cada línea guarda la sección de la estación que pasa por su corte con el
//! borde de ataque: de ahí sale c1m = Q_La / A1 «según la posición del borde»
//! (Gülich, ec. T3.1.2, p. 83).
//!
//! El plano de entrada y la salida se fuerzan como primera y última estación
//! aunque las paredes no arranquen axiales ni acaben radiales. Si el cubo
//! empieza aguas abajo de la tapa o acaba antes que ella, las estaciones que
//! no tienen cuasi-normal salen en abanico desde el extremo del cubo.
//!
//! Supuesto declarado: las cuasi-normales son rectas y cm es constante sobre
//! cada una. Srinivasan dice que corregir las normales con más precisión no
//! cambia el rodete.
//!
//! Las líneas no entran en las curvas del modelo: ni su huella ni su CSV
//! cambian. El reparto sobre cada estación es cerrado; el punto del cubo de
//! cada cuasi-normal se busca por bisección.

use thiserror::Error;

use crate::blade_setup::STREAMLINE_COUNTS;
use crate::meridian::{MeridionalModel, arc_fraction, cumulative};

/// Pasos de la bisección que busca el punto del cubo de una cuasi-normal. De
/// hmflow: deja el error muy por debajo de una micra en un cubo de metros.
const NORMAL_BISECTION_STEPS: usize = 48;

/// Holgura, en fracción de tramo, con que un corte en el extremo de un tramo
/// cuenta como corte. De hmflow: evita perder un cruce que cae en un vértice.
const CROSSING_SLACK: f64 = 1.0e-9;

/// Una línea de corriente del meridiano, en mm.
#[derive(Debug, Clone, PartialEq)]
pub struct Streamline {
    /// Fracción de caudal entre el cubo y la línea: 0 en el cubo, 1 en la tapa.
    pub psi: f64,
    /// `[z, r]`, del plano de entrada a d2; un punto por estación del canal.
    pub points: Vec<[f64; 2]>,
    /// El corte con el borde de ataque, `[z, r]`.
    pub leading_edge: [f64; 2],
    /// Longitud sobre la línea desde su primer punto hasta ese corte.
    pub m_leading_edge: f64,
    /// Sección de paso, mm², de la estación que pasa por el corte: 2π·r·b,
    /// interpolada entre las dos estaciones que lo encierran.
    pub leading_edge_area: f64,
}

impl Streamline {
    /// sen γ en el corte con el borde de ataque: |dr/dm| del tramo que lo
    /// contiene (el último si cae en el último punto). γ es el ángulo de la
    /// línea con el eje: 1 en un tramo radial y 0 en uno axial. Sin tramos, 1.
    #[must_use]
    pub fn sin_gamma_at_leading_edge(&self) -> f64 {
        let lengths = cumulative(&self.points);
        let mut sine = 1.0;
        for (index, pair) in self.points.windows(2).enumerate() {
            let length = lengths[index + 1] - lengths[index];
            if length <= 0.0 {
                continue;
            }
            sine = ((pair[1][1] - pair[0][1]) / length).abs().min(1.0);
            if self.m_leading_edge <= lengths[index + 1] {
                break;
            }
        }
        sine
    }
}

/// Por qué no hay líneas de corriente.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StreamlineError {
    /// La línea no corta la curva «Borde de ataque». No se inventa un punto.
    #[error("la línea de corriente {line} no corta el borde de ataque")]
    NoLeadingEdge { line: usize },
    /// Faltan las curvas de la tapa delantera o de la trasera.
    #[error("faltan las curvas de la tapa delantera o de la trasera")]
    NoChannel,
}

/// La posición t ∈ [0, 1] sobre la recta del cubo a la tapa en la que queda la
/// fracción de caudal `psi` entre el cubo y la línea.
///
/// Sobre esa recta r es lineal en t, así que F(t) = ∫₀ᵗ r dt' =
/// r_H·t + (r_S − r_H)·t²/2, con F₁ = F(1) = (r_H + r_S)/2. La línea está donde
/// F(t) = ψ·F₁. Se resuelve en la forma que no pierde precisión cuando
/// r_S ≈ r_H:
///
/// t = 2·ψ·F₁ / (r_H + √(r_H² + 2·(r_S − r_H)·ψ·F₁))
///
/// Con r_S = r_H es t = ψ (reparto uniforme) y con el cubo en el eje, t = √ψ.
/// La anchura de la estación se cancela. Con los dos radios nulos no hay
/// caudal que repartir y se devuelve ψ.
#[must_use]
pub fn equal_flow_fraction(r_hub: f64, r_shroud: f64, psi: f64) -> f64 {
    let psi = psi.clamp(0.0, 1.0);
    let whole = (r_hub + r_shroud) / 2.0;
    let root = (r_hub * r_hub + 2.0 * (r_shroud - r_hub) * psi * whole)
        .max(0.0)
        .sqrt();
    let denominator = r_hub + root;
    if denominator <= 0.0 {
        return psi;
    }
    (2.0 * psi * whole / denominator).clamp(0.0, 1.0)
}

/// Una estación del canal, en mm: la recta del punto del cubo al de la tapa.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuasiNormal {
    /// `[z, r]` sobre la tapa trasera.
    pub hub: [f64; 2],
    /// `[z, r]` sobre la tapa delantera.
    pub shroud: [f64; 2],
}

impl QuasiNormal {
    /// Anchura del canal sobre la estación, b.
    #[must_use]
    pub fn width(&self) -> f64 {
        (self.shroud[0] - self.hub[0]).hypot(self.shroud[1] - self.hub[1])
    }

    /// Sección de paso, 2π·r·b con r el radio del punto medio: el área del
    /// tronco de cono que genera la estación (Gülich 2020, §7.2.2.1, p. 418).
    #[must_use]
    pub fn area(&self) -> f64 {
        std::f64::consts::PI * (self.hub[1] + self.shroud[1]) * self.width()
    }
}

/// Las estaciones cuasi-normales del canal, una por punto de la tapa
/// delantera, del plano de entrada a la salida.
///
/// Para cada punto S de la tapa delantera se busca el punto H del cubo en el
/// que la recta H–S forma el mismo ángulo con las dos paredes:
/// (H − S)·(t_S + t_H) = 0, con t las tangentes unitarias. Es la cuerda entre
/// los puntos de contacto del círculo inscrito y es perpendicular a la
/// bisectriz de las dos paredes, la dirección media del flujo. La primera y la
/// última estación son los extremos de las dos curvas, y el punto del cubo
/// nunca retrocede, así que las estaciones no se cruzan.
#[must_use]
pub fn quasi_normals(model: &MeridionalModel) -> Vec<QuasiNormal> {
    let (Some(front), Some(rear)) = (model.curve("Tapa delantera"), model.curve("Tapa trasera"))
    else {
        return Vec::new();
    };
    normals_between(&front.points, &rear.points)
}

/// Las cuasi-normales entre dos paredes dadas como polilíneas `[z, r]`.
fn normals_between(front: &[[f64; 2]], rear: &[[f64; 2]]) -> Vec<QuasiNormal> {
    if front.len() < 2 || rear.len() < 2 {
        return Vec::new();
    }
    let rear_wall = Wall::new(rear);
    let front_tangents = vertex_tangents(front);
    let last = front.len() - 1;
    let mut reached = 0.0;
    front
        .iter()
        .zip(&front_tangents)
        .enumerate()
        .map(|(index, (shroud, tangent))| {
            reached = if index == 0 {
                0.0
            } else if index == last {
                rear_wall.total()
            } else {
                rear_wall.partner(*shroud, *tangent, reached)
            };
            QuasiNormal {
                hub: rear_wall.at(reached).0,
                shroud: *shroud,
            }
        })
        .collect()
}

/// Una pared como polilínea con su longitud acumulada y sus tangentes.
struct Wall<'a> {
    points: &'a [[f64; 2]],
    lengths: Vec<f64>,
    tangents: Vec<[f64; 2]>,
}

impl<'a> Wall<'a> {
    fn new(points: &'a [[f64; 2]]) -> Self {
        Self {
            points,
            lengths: cumulative(points),
            tangents: vertex_tangents(points),
        }
    }

    fn total(&self) -> f64 {
        self.lengths[self.lengths.len() - 1]
    }

    /// El punto y la tangente unitaria a la longitud `length` desde el
    /// principio. En los extremos, el vértice exacto.
    fn at(&self, length: f64) -> ([f64; 2], [f64; 2]) {
        let last = self.points.len() - 1;
        if length <= 0.0 {
            return (self.points[0], self.tangents[0]);
        }
        if length >= self.total() {
            return (self.points[last], self.tangents[last]);
        }
        let index = self
            .lengths
            .partition_point(|reached| *reached <= length)
            .clamp(1, last)
            - 1;
        let span = self.lengths[index + 1] - self.lengths[index];
        let fraction = if span > 0.0 {
            (length - self.lengths[index]) / span
        } else {
            0.0
        };
        let blend = |a: [f64; 2], b: [f64; 2]| {
            [
                a[0] + fraction * (b[0] - a[0]),
                a[1] + fraction * (b[1] - a[1]),
            ]
        };
        (
            blend(self.points[index], self.points[index + 1]),
            unit(blend(self.tangents[index], self.tangents[index + 1])),
        )
    }

    /// (H − S)·(t_S + t_H) en el punto H de esta pared a la longitud `length`:
    /// negativo si H se queda atrás, positivo si se adelanta.
    fn misalignment(&self, shroud: [f64; 2], tangent: [f64; 2], length: f64) -> f64 {
        let (hub, hub_tangent) = self.at(length);
        (hub[0] - shroud[0]) * (tangent[0] + hub_tangent[0])
            + (hub[1] - shroud[1]) * (tangent[1] + hub_tangent[1])
    }

    /// La longitud, de `from` en adelante, del punto que hace cuasi-normal la
    /// recta hasta `shroud`. Se toma el primer cambio de signo recorriendo los
    /// vértices y se afina por bisección. Si desde `from` ya se adelanta, se
    /// queda en `from`; si nunca alcanza, en el final de la pared.
    fn partner(&self, shroud: [f64; 2], tangent: [f64; 2], from: f64) -> f64 {
        if self.misalignment(shroud, tangent, from) >= 0.0 {
            return from;
        }
        let mut low = from;
        let Some(mut high) = self
            .lengths
            .iter()
            .copied()
            .filter(|length| *length > from)
            .find(|length| {
                let ahead = self.misalignment(shroud, tangent, *length) >= 0.0;
                if !ahead {
                    low = *length;
                }
                ahead
            })
        else {
            return self.total();
        };
        for _ in 0..NORMAL_BISECTION_STEPS {
            let middle = (low + high) / 2.0;
            if self.misalignment(shroud, tangent, middle) >= 0.0 {
                high = middle;
            } else {
                low = middle;
            }
        }
        (low + high) / 2.0
    }
}

/// La tangente unitaria en cada vértice: la dirección entre sus dos vecinos.
fn vertex_tangents(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let last = points.len().saturating_sub(1);
    (0..points.len())
        .map(|index| {
            let (before, after) = (
                points[index.saturating_sub(1)],
                points[(index + 1).min(last)],
            );
            unit([after[0] - before[0], after[1] - before[1]])
        })
        .collect()
}

/// El vector con módulo 1; el nulo se deja como está.
fn unit(vector: [f64; 2]) -> [f64; 2] {
    let length = vector[0].hypot(vector[1]);
    if length > 0.0 {
        [vector[0] / length, vector[1] / length]
    } else {
        vector
    }
}

/// La sección de la estación que pasa por el punto a la longitud `m` de una
/// línea: la de las dos estaciones que lo encierran, interpolada. `lengths`
/// es la longitud acumulada de la línea, con un punto por estación.
fn area_at(areas: &[f64], lengths: &[f64], m: f64) -> f64 {
    let last = areas.len() - 1;
    let index = lengths
        .partition_point(|reached| *reached <= m)
        .clamp(1, last)
        - 1;
    let span = lengths[index + 1] - lengths[index];
    let fraction = if span > 0.0 {
        ((m - lengths[index]) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    areas[index] + fraction * (areas[index + 1] - areas[index])
}

/// Las `count` líneas de corriente del modelo, del cubo (ψ = 0) a la tapa
/// (ψ = 1), con ψ_k = k/(N − 1).
///
/// La primera y la última son las propias paredes y su corte es el extremo
/// del borde de ataque que ya tiene el modelo, sin intersección numérica. En
/// las intermedias el corte es el primer cruce de la línea con la curva
/// «Borde de ataque», recorriendo la línea desde el plano de entrada.
pub fn streamlines(model: &MeridionalModel, count: u8) -> Result<Vec<Streamline>, StreamlineError> {
    debug_assert!(
        STREAMLINE_COUNTS.contains(&count),
        "número de líneas de corriente fuera de la lista: {count}"
    );
    let stations = quasi_normals(model);
    if stations.len() < 2 {
        return Err(StreamlineError::NoChannel);
    }
    let areas: Vec<f64> = stations.iter().map(QuasiNormal::area).collect();
    let count = usize::from(count.max(2));
    let last = count - 1;
    let edge: &[[f64; 2]] = model
        .curve("Borde de ataque")
        .map_or(&[], |curve| curve.points.as_slice());
    // `edge_points` es [tapa, cubo].
    let [shroud_cut, hub_cut] = model.params.edge_points;

    (0..count)
        .map(|line| {
            let psi = line as f64 / last as f64;
            let points: Vec<[f64; 2]> = stations
                .iter()
                .map(|station| {
                    let (hub, shroud) = (station.hub, station.shroud);
                    if line == 0 {
                        hub
                    } else if line == last {
                        shroud
                    } else {
                        let t = equal_flow_fraction(hub[1], shroud[1], psi);
                        [
                            hub[0] + t * (shroud[0] - hub[0]),
                            hub[1] + t * (shroud[1] - hub[1]),
                        ]
                    }
                })
                .collect();
            let lengths = cumulative(&points);
            let total = lengths[lengths.len() - 1];
            let (leading_edge, m_leading_edge) = if line == 0 {
                (hub_cut, arc_fraction(&points, hub_cut) * total)
            } else if line == last {
                (shroud_cut, arc_fraction(&points, shroud_cut) * total)
            } else {
                let (index, fraction, point) =
                    first_crossing(&points, edge).ok_or(StreamlineError::NoLeadingEdge { line })?;
                let m = lengths[index] + fraction * (lengths[index + 1] - lengths[index]);
                (point, m)
            };
            Ok(Streamline {
                psi,
                points,
                leading_edge,
                m_leading_edge,
                leading_edge_area: area_at(&areas, &lengths, m_leading_edge),
            })
        })
        .collect()
}

/// El primer cruce de `line` con `edge`, recorriendo `line` desde su primer
/// punto: el índice del tramo de `line`, la fracción dentro de él y el punto.
/// Si el borde corta dos veces la misma línea, gana el corte más cercano al
/// principio de la línea.
fn first_crossing(line: &[[f64; 2]], edge: &[[f64; 2]]) -> Option<(usize, f64, [f64; 2])> {
    for (index, pair) in line.windows(2).enumerate() {
        let earliest = edge
            .windows(2)
            .filter_map(|other| segment_crossing(pair[0], pair[1], other[0], other[1]))
            .min_by(f64::total_cmp);
        if let Some(fraction) = earliest {
            let point = [
                pair[0][0] + fraction * (pair[1][0] - pair[0][0]),
                pair[0][1] + fraction * (pair[1][1] - pair[0][1]),
            ];
            return Some((index, fraction, point));
        }
    }
    None
}

/// La fracción del tramo `a`–`b` en la que lo corta el tramo `c`–`d`, si se
/// cortan. Dos tramos paralelos no se cortan, aunque se solapen.
fn segment_crossing(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Option<f64> {
    let (ab, cd) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
    let cross = ab[0] * cd[1] - ab[1] * cd[0];
    let scale = ab[0].hypot(ab[1]) * cd[0].hypot(cd[1]);
    if scale <= 0.0 || cross.abs() <= f64::EPSILON * scale {
        return None;
    }
    let ac = [c[0] - a[0], c[1] - a[1]];
    let on_ab = (ac[0] * cd[1] - ac[1] * cd[0]) / cross;
    let on_cd = (ac[0] * ab[1] - ac[1] * ab[0]) / cross;
    let inside = |value: f64| (-CROSSING_SLACK..=1.0 + CROSSING_SLACK).contains(&value);
    (inside(on_ab) && inside(on_cd)).then(|| on_ab.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una pared axial de radio `r`, con un punto cada 10 mm de `from` a `to`.
    fn axial_wall(r: f64, from: u32, to: u32) -> Vec<[f64; 2]> {
        (from..=to).step_by(10).map(|z| [f64::from(z), r]).collect()
    }

    fn assert_sound(stations: &[QuasiNormal], rear: &[[f64; 2]]) {
        let mut reached = f64::NEG_INFINITY;
        for station in stations {
            assert!(station.area().is_finite() && station.area() > 0.0);
            // El cubo es axial: avanzar por él es crecer en z.
            assert!(station.hub[0] >= reached, "el cubo retrocede: {stations:?}");
            reached = station.hub[0];
            assert!(station.hub[0] >= rear[0][0] && station.hub[0] <= rear[rear.len() - 1][0]);
        }
    }

    #[test]
    fn between_parallel_walls_the_normals_are_radial() {
        let (front, rear) = (axial_wall(80.0, 0, 100), axial_wall(30.0, 0, 100));
        let stations = normals_between(&front, &rear);
        assert_eq!(stations.len(), front.len());
        for station in &stations {
            assert!((station.hub[0] - station.shroud[0]).abs() < 1e-9);
            assert!((station.width() - 50.0).abs() < 1e-9);
        }
        assert_sound(&stations, &rear);
    }

    /// Un cubo que empieza aguas abajo de la tapa: las primeras estaciones no
    /// tienen cuasi-normal y salen en abanico desde el arranque del cubo.
    #[test]
    fn a_hub_that_starts_later_fans_out_from_its_first_point() {
        let (front, rear) = (axial_wall(80.0, 0, 100), axial_wall(30.0, 40, 100));
        let stations = normals_between(&front, &rear);
        for station in &stations[..4] {
            assert_eq!(station.hub, rear[0], "{station:?}");
        }
        for station in &stations[5..] {
            assert!((station.hub[0] - station.shroud[0]).abs() < 1e-9);
        }
        assert_sound(&stations, &rear);
    }

    /// Un cubo que acaba antes que la tapa: las últimas estaciones salen en
    /// abanico desde su final.
    #[test]
    fn a_hub_that_ends_early_fans_out_from_its_last_point() {
        let (front, rear) = (axial_wall(80.0, 0, 100), axial_wall(30.0, 0, 60));
        let stations = normals_between(&front, &rear);
        let end = rear[rear.len() - 1];
        for station in &stations[7..] {
            assert_eq!(station.hub, end, "{station:?}");
        }
        for station in &stations[..6] {
            assert!((station.hub[0] - station.shroud[0]).abs() < 1e-9);
        }
        assert_sound(&stations, &rear);
    }

    /// Un vértice repetido tres veces en el cubo no da tramos sin tangente
    /// que rompan la búsqueda.
    #[test]
    fn a_repeated_vertex_on_the_hub_changes_nothing() {
        let front = axial_wall(80.0, 0, 100);
        let mut rear = axial_wall(30.0, 0, 100);
        rear.insert(5, [40.0, 30.0]);
        rear.insert(5, [40.0, 30.0]);
        let stations = normals_between(&front, &rear);
        for station in &stations {
            assert!(
                (station.hub[0] - station.shroud[0]).abs() < 1e-9,
                "{station:?}"
            );
        }
        assert_sound(&stations, &rear);
    }

    #[test]
    fn a_wall_of_one_point_has_no_stations() {
        assert!(normals_between(&[[0.0, 80.0]], &axial_wall(30.0, 0, 100)).is_empty());
        assert!(normals_between(&axial_wall(80.0, 0, 100), &[[0.0, 30.0]]).is_empty());
    }

    #[test]
    fn an_edge_that_crosses_twice_gives_the_first_cut_along_the_line() {
        let line = [[0.0, 0.0], [10.0, 0.0]];
        let edge = [[2.0, -1.0], [2.0, 1.0], [6.0, 1.0], [6.0, -1.0]];
        let (index, fraction, point) = first_crossing(&line, &edge).unwrap();
        assert_eq!(index, 0);
        assert!((fraction - 0.2).abs() < 1e-15, "{fraction}");
        assert!((point[0] - 2.0).abs() < 1e-15 && point[1].abs() < 1e-15);
        // El orden de los tramos del borde no cambia cuál es el primero.
        let reversed: Vec<[f64; 2]> = edge.iter().rev().copied().collect();
        let (_, backwards, _) = first_crossing(&line, &reversed).unwrap();
        assert!((backwards - 0.2).abs() < 1e-15, "{backwards}");
    }

    #[test]
    fn the_cut_is_found_on_a_later_segment_of_the_line() {
        let line = [[0.0, 0.0], [4.0, 0.0], [8.0, 0.0]];
        let edge = [[6.0, -1.0], [6.0, 1.0]];
        let (index, fraction, point) = first_crossing(&line, &edge).unwrap();
        assert_eq!(index, 1);
        assert!((fraction - 0.5).abs() < 1e-15);
        assert_eq!(point, [6.0, 0.0]);
    }

    #[test]
    fn a_cut_on_a_vertex_of_the_line_is_not_lost() {
        let line = [[0.0, 0.0], [4.0, 0.0], [8.0, 0.0]];
        let edge = [[4.0, -1.0], [4.0, 1.0]];
        let (index, fraction, _) = first_crossing(&line, &edge).unwrap();
        assert_eq!((index, fraction), (0, 1.0));
    }

    #[test]
    fn an_edge_that_stops_short_or_runs_parallel_does_not_cut() {
        let line = [[0.0, 0.0], [10.0, 0.0]];
        assert_eq!(first_crossing(&line, &[[2.0, 1.0], [2.0, 3.0]]), None);
        assert_eq!(first_crossing(&line, &[[0.0, 1.0], [10.0, 1.0]]), None);
        assert_eq!(first_crossing(&line, &[[1.0, 0.0], [5.0, 0.0]]), None);
        assert_eq!(first_crossing(&line, &[]), None);
    }
}
