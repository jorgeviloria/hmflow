//! Líneas de corriente del corte meridional.
//!
//! El canal se reparte en N − 1 canales parciales de igual caudal. Con cm
//! constante a lo largo de cada estación, igual caudal es igual ∫r dl entre
//! líneas vecinas: ΔQ = 2π·r·Δb·cm (Gülich, *Centrifugal Pumps*, 4.ª ed., 2020,
//! §7.2.1, método de Kaplan, paso 1, p. 419, «frecuentemente con la hipótesis
//! cm = constante»; Srinivasan, *Rotodynamic Pumps*, 2.ª ed., ec. 4.63,
//! r·Δb = cte).
//!
//! Las estaciones son las de [`MeridionalModel::channel_from`] con (0, 0): el
//! canal entero desde el plano de entrada, porque el borde de ataque puede
//! meterse en el ojo y las líneas tienen que llegar a él. Cada estación es la
//! recta del punto del cubo al de la tapa.
//!
//! Supuesto declarado: esas rectas no son normales exactas a las líneas.
//! Gülich habla de normales «estimadas» (p. 419) y Srinivasan dice que
//! corregirlas con precisión no cambia el rodete.
//!
//! Las líneas no entran en las curvas del modelo: ni su huella ni su CSV
//! cambian. El cálculo es cerrado por estación, sin iteración.

use thiserror::Error;

use crate::blade_setup::STREAMLINE_COUNTS;
use crate::meridian::{MeridionalModel, arc_fraction, cumulative};

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
    let stations = model.channel_from(0.0, 0.0);
    if stations.len() < 2 {
        return Err(StreamlineError::NoChannel);
    }
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
                    let (hub, shroud) = (station.hub_point, station.shroud_point);
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
