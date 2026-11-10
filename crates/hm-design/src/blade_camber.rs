//! La línea media del álabe sobre cada línea de corriente (álabe de doble
//! curvatura, forma «Libre 3D»).
//!
//! Sobre la línea k, m es la longitud de la polilínea `[z, r]`. El álabe
//! ocupa de `m_leading_edge` al último punto: L_k = m_TE − m_LE, y la fracción
//! es x = (m − m_LE)/L_k, de 0 en el borde de ataque a 1 en la salida.
//!
//! - **Nodos.** J + 1 nodos a pasos iguales de m, J = [`CAMBER_STEPS`]: el paso
//!   2 del método de Kaplan, que divide cada línea en elementos de igual Δm
//!   (Gülich, *Centrifugal Pumps*, 4.ª ed., 2020, §7.2.2.2, p. 419). Los 200 son
//!   de hmflow. (z, r) se interpola linealmente en el tramo de la polilínea.
//! - **Ley de ángulo.** β(x) = β1B,k + (β2B − β1B,k)·x + desviación cúbica con
//!   manijas en 1/3 y 2/3. La recta es la de Gülich, ecs. 7.8 y 7.9, p. 420; la
//!   forma cúbica es `geometry::beta_profile`, de hmflow; las manijas de la
//!   línea son (1 − ψ)·cubo + ψ·tapa, de hmflow ([`CamberBows::at`]). β es el
//!   ángulo verdadero con la dirección periférica (fig. 7.7c, p. 419).
//! - **De β a θ.** dθ = dm/(r·tan β) (Gülich, ec. 7.7, p. 419: Δu = Δm/tan β),
//!   por trapecios en m, con el mismo orden de operaciones que `wrap_angle` de
//!   `geometry.rs`. No lleva sen γ: sobre un tramo axial la línea media es un
//!   arco de hélice.
//! - **Magnitudes de la misma pasada.** m′ = ∫dm/r, la transformación conforme
//!   de la superficie de revolución (ds² = r²·(dm′² + dθ²); Srinivasan,
//!   *Rotodynamic Pumps*, 2.ª ed., ec. 4.61), en la que la pendiente
//!   dm′/dθ = tan β; y la longitud verdadera Σ√(Δm² + (r̄·Δθ)²) (Gülich,
//!   ec. 7.6, p. 419).
//! - **Borde de salida común.** Con Θ = máx ε_k, la línea k se desplaza para
//!   acabar en θ = Θ: sin torsión a la salida (Gülich, p. 424). El borde de
//!   ataque en planta no se impone, es un resultado: θ_LE,k = Θ − ε_k. El
//!   convenio de origen (θ = 0 en el punto más adelantado) es de hmflow.
//!
//! Todo en SI y radianes; las líneas de corriente llegan en mm. Solo se lee el
//! meridiano: no cambia ni un número del 1D.

use thiserror::Error;

use crate::blade_setup::{BladeShape, CamberBows};
use crate::geometry::{BLADE_BETA_MIN_DEG, beta_profile};
use crate::inlet_triangles::InletTriangle;
use crate::meridian::cumulative;
use crate::streamlines::Streamline;

/// Tramos de igual Δm de cada línea media. De hmflow, igual que
/// `BLADE_STEPS_SHAPED`.
pub const CAMBER_STEPS: usize = 200;

/// Radio por debajo del cual un nodo cuenta como «en el eje», m. De hmflow.
pub const CAMBER_MIN_RADIUS: f64 = 1.0e-6;

/// Un nodo de la línea media de una línea de corriente.
#[derive(Debug, Clone, PartialEq)]
pub struct CamberPoint {
    /// Fracción de la longitud meridional del álabe: 0 entrada, 1 salida.
    pub x: f64,
    /// Longitud meridional desde el nodo 0 de la línea, m.
    pub m: f64,
    /// m.
    pub z: f64,
    /// m.
    pub r: f64,
    /// rad; crece de la entrada a la salida y la salida de todas las líneas
    /// está en Θ.
    pub theta: f64,
    /// rad, ángulo verdadero con la dirección periférica.
    pub beta: f64,
    /// m′ = ∫dm/r desde el nodo 0, adimensional.
    pub m_conformal: f64,
}

/// De dónde sale la línea media de una línea de corriente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CamberOrigin {
    /// β impuesto por la ley e integrado.
    Integrated,
    /// Sale de las rectas cubo–tapa; β medido.
    Ruled,
}

/// La línea media de una línea de corriente.
#[derive(Debug, Clone, PartialEq)]
pub struct CamberLine {
    pub psi: f64,
    pub origin: CamberOrigin,
    /// J + 1 nodos, de la entrada a la salida.
    pub points: Vec<CamberPoint>,
    /// rad, ε_k.
    pub wrap: f64,
    /// m, longitud verdadera.
    pub length: f64,
    /// m, desvío del borde de ataque; 0 en las integradas.
    pub edge_gap: f64,
}

/// La línea media de todas las líneas de corriente.
#[derive(Debug, Clone, PartialEq)]
pub struct BladeCamber {
    /// `Free3D` o `Ruled3D`.
    pub shape: BladeShape,
    /// Del cubo a la tapa, como las líneas de corriente.
    pub lines: Vec<CamberLine>,
    /// rad, Θ.
    pub wrap_max: f64,
}

/// Por qué no sale la línea media. Ninguno se arregla en silencio.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CamberError {
    #[error(
        "la forma del álabe no es 3D: la línea media por línea de corriente solo existe para «Libre 3D» y «Reglada 3D»"
    )]
    NotThreeD,

    #[error("hacen falta al menos dos líneas de corriente y un triángulo de entrada por línea")]
    LineMismatch,

    #[error(
        "línea de corriente {line} (0 es el cubo): el corte con el borde de ataque cae al final de la línea y no queda longitud de álabe"
    )]
    NoLength { line: usize },

    #[error(
        "línea de corriente {line} (0 es el cubo): pasa a menos de 1e-6 m del eje y θ = ∫dm/(r·tan β) no está definido"
    )]
    OnAxis { line: usize },

    #[error(
        "línea de corriente {line} (0 es el cubo): β = {:.2}° a x = {x:.3} queda fuera de los límites del álabe",
        .beta.to_degrees()
    )]
    AngleOutOfRange { line: usize, x: f64, beta: f64 },

    #[error(
        "línea de corriente {line} (0 es el cubo): la recta de la estación {station} no corta la línea, o cubo y tapa difieren 180° o más"
    )]
    RulingMisses { line: usize, station: usize },
}

/// La línea media 3D de un álabe sobre sus líneas de corriente.
///
/// `lines` llegan en mm, del cubo a la tapa; `triangles` lleva uno por línea
/// con su β1B; `beta2b` es el β2 del 1D, común a todas. `Ruled3D` llega en la
/// tarea siguiente y de momento da [`CamberError::NotThreeD`].
pub fn blade_camber(
    shape: BladeShape,
    lines: &[Streamline],
    triangles: &[InletTriangle],
    beta2b: f64,
    bows: &CamberBows,
) -> Result<BladeCamber, CamberError> {
    match shape {
        BladeShape::Free3D => {}
        _ => return Err(CamberError::NotThreeD),
    }
    if lines.len() < 2 || lines.len() != triangles.len() {
        return Err(CamberError::LineMismatch);
    }

    let mut integrated = Vec::with_capacity(lines.len());
    for (index, (line, triangle)) in lines.iter().zip(triangles).enumerate() {
        integrated.push(integrate_line(index, line, triangle, beta2b, bows)?);
    }

    // Borde de salida común: todas acaban en Θ = máx ε.
    let wrap_max = integrated
        .iter()
        .map(|line| line.wrap)
        .fold(f64::NEG_INFINITY, f64::max);
    for line in &mut integrated {
        let shift = wrap_max - line.wrap;
        for point in &mut line.points {
            point.theta += shift;
        }
    }

    Ok(BladeCamber {
        shape,
        lines: integrated,
        wrap_max,
    })
}

/// Integra la línea media de una línea de corriente, con θ = 0 en el nodo 0.
fn integrate_line(
    index: usize,
    line: &Streamline,
    triangle: &InletTriangle,
    beta2b: f64,
    bows: &CamberBows,
) -> Result<CamberLine, CamberError> {
    let polyline: Vec<[f64; 2]> = line
        .points
        .iter()
        .map(|point| [point[0] / 1000.0, point[1] / 1000.0])
        .collect();
    let lengths = cumulative(&polyline);
    let total = lengths.last().copied().unwrap_or(0.0);
    let m_start = line.m_leading_edge / 1000.0;
    let span = total - m_start;
    if span.is_nan() || span <= 0.0 {
        return Err(CamberError::NoLength { line: index });
    }

    // Nodos a pasos iguales de m, con (z, r) interpolado en su tramo.
    let steps = CAMBER_STEPS;
    let mut nodes: Vec<(f64, f64, f64)> = Vec::with_capacity(steps + 1);
    let mut segment = 0;
    for j in 0..=steps {
        let m = if j == steps {
            total
        } else {
            m_start + j as f64 * span / steps as f64
        };
        while segment + 2 < polyline.len() && lengths[segment + 1] < m {
            segment += 1;
        }
        let (a, b) = (polyline[segment], polyline[segment + 1]);
        let run = lengths[segment + 1] - lengths[segment];
        let t = if run > 0.0 {
            ((m - lengths[segment]) / run).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (z, r) = (a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]));
        if r < CAMBER_MIN_RADIUS || r.is_nan() {
            return Err(CamberError::OnAxis { line: index });
        }
        nodes.push((z, r, m));
    }

    // Ley de ángulo en cada nodo y sus límites.
    let bow = bows.at(line.psi);
    let has_bow = bow.0 != 0.0 || bow.1 != 0.0;
    let lower = BLADE_BETA_MIN_DEG.to_radians();
    let upper = std::f64::consts::FRAC_PI_2;
    let mut betas = Vec::with_capacity(steps + 1);
    for j in 0..=steps {
        let x = j as f64 / steps as f64;
        let beta = beta_profile(triangle.beta1b, beta2b, bow, x);
        let checked = has_bow || j == 0 || j == steps;
        let floor = if has_bow { lower } else { 0.0 };
        let inside = if has_bow {
            beta >= floor && beta < upper
        } else {
            beta > floor && beta < upper
        };
        if checked && !inside {
            return Err(CamberError::AngleOutOfRange {
                line: index,
                x,
                beta,
            });
        }
        betas.push(beta);
    }

    // θ, m′ y longitud verdadera por trapecios en m.
    let mut points = Vec::with_capacity(steps + 1);
    let mut theta = 0.0;
    let mut conformal = 0.0;
    let mut length = 0.0;
    for j in 0..=steps {
        let (z, r, m) = nodes[j];
        if j > 0 {
            let (_, r0, m0) = nodes[j - 1];
            let dm = m - m0;
            let g0 = 1.0 / (r0 * betas[j - 1].tan());
            let g1 = 1.0 / (r * betas[j].tan());
            let d_theta = 0.5 * (g0 + g1) * dm;
            theta += d_theta;
            conformal += 0.5 * (1.0 / r0 + 1.0 / r) * dm;
            length += dm.hypot(0.5 * (r0 + r) * d_theta);
        }
        points.push(CamberPoint {
            x: j as f64 / steps as f64,
            m: m - m_start,
            z,
            r,
            theta,
            beta: betas[j],
            m_conformal: conformal,
        });
    }

    Ok(CamberLine {
        psi: line.psi,
        origin: CamberOrigin::Integrated,
        points,
        wrap: theta,
        length,
        edge_gap: 0.0,
    })
}
