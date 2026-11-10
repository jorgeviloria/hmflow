//! Triángulo de entrada en cada línea de corriente.
//!
//! En el corte de la línea k con el borde de ataque, de radio r_k:
//!
//! - u1,k = ω·r_k (Gülich, *Centrifugal Pumps*, 4.ª ed., 2020, tabla 3.1).
//! - c1m,k = Q_La / A1,k. Q_La es el caudal del rodete, con fugas, y A1,k la
//!   sección de paso 2π·r·b de la estación cuasi-normal que pasa por el corte
//!   (ec. T3.1.2, p. 83, «A1 según la posición del borde de ataque»; las
//!   secciones A = 2π·r·b, §7.2.2.1, p. 418). No es la superficie que genera
//!   el borde: un borde inclinado respecto a las normales tiene más superficie
//!   que sección de paso, y con ella c1m saldría bajo.
//! - β′1,k = arctan(τ1,k·c1m / u1,k), sin prerrotación, y β1B,k = β′1,k + i1
//!   (§7.2.1, p. 411: el cálculo se hace en varias líneas de corriente). τ1,k
//!   es el bloqueo de la ec. T3.1.7 con el diámetro 2·r_k, y se itera con β1B,k
//!   en la misma función que usa el 1D ([`crate::leading_edge`]), con el mismo
//!   espesor de álabe.
//!
//! Junto al β1B que pide el flujo va el ángulo que el álabe 2D de hoy tiene
//! sobre esa línea. La línea media 2D da β en el plano radial, porque θ sale de
//! dθ = dr / (r·tan β). Sobre una línea inclinada γ con el eje, Δr = Δm·sen γ, y
//! con Δu = Δm / tan β (ec. 7.7, p. 419) y Δu = r·Δθ queda
//! tan β = tan β_planta(r_k) / sen γ_k. Esta relación es de hmflow. El álabe 2D
//! empieza en d1/2: un corte por debajo no tiene álabe. En un tramo axial
//! (sen γ → 0) dr = 0 y el ángulo en planta no dice qué ángulo tiene el álabe
//! sobre la línea: no se compara.
//!
//! Supuestos declarados:
//!
//! - λ_La = 90°: el álabe es perpendicular a la tapa en el borde de ataque. El
//!   ángulo real solo se conoce con el álabe 3D. El 1D ya lo supone.
//! - cm constante sobre cada estación: cm aguas arriba del borde es «casi
//!   siempre constante en la sección de entrada» (Gülich, p. 411). Un borde
//!   que cruza varias estaciones toma en cada línea el cm de la suya; si el
//!   borde sigue una estación, c1m es el mismo en todas. Dar un c1m por
//!   línea es de hmflow: Gülich dibuja un solo A1, el del círculo inscrito en
//!   el borde (fig. 7.4), y solo admite otro cm por línea según el bloqueo
//!   local del cubo.
//!
//! Nada de esto cambia el 1D: su β1B sigue siendo el de d1.

use hm_formula::Registry;

use crate::geometry::{BladePoint, ImpellerGeometry};
use crate::streamlines::Streamline;
use crate::{DesignError, DesignResult, leading_edge, mechanics};

/// |Δβ| a partir del cual la tabla lo colorea, en grados. De hmflow: el ancho
/// del intervalo de incidencia de 0 a 4° de Gülich 2020, p. 411.
pub const INLET_MISMATCH_WARN_DEG: f64 = 4.0;

/// sen γ por debajo del cual el tramo se trata como axial. De hmflow.
pub const AXIAL_SIN_GAMMA: f64 = 1.0e-3;

/// Holgura, en m, con que un corte se considera en d1/2. De hmflow.
pub const BLADE_START_TOLERANCE: f64 = 1.0e-9;

/// mm² por m² y mm por m: el corte meridional va en mm y el 1D en SI.
const MM2_PER_M2: f64 = 1.0e6;
const MM_PER_M: f64 = 1.0e3;

/// El ángulo del álabe 2D actual en el corte de una línea con el borde.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Blade2dAngle {
    /// Ángulo sobre la línea de corriente, rad.
    Angle(f64),
    /// El corte queda por debajo de d1/2: el álabe 2D no llega.
    NoBlade,
    /// La línea es axial en el corte (sen γ < [`AXIAL_SIN_GAMMA`]): el ángulo
    /// en planta no se puede llevar a la línea y no hay nada que comparar.
    Axial,
}

/// El triángulo de entrada de una línea de corriente.
#[derive(Debug, Clone, PartialEq)]
pub struct InletTriangle {
    /// Fracción de caudal de la línea: 0 en el cubo, 1 en la tapa.
    pub psi: f64,
    /// Radio del corte con el borde de ataque, m.
    pub r1: f64,
    /// Velocidad periférica en el corte, m/s.
    pub u1: f64,
    /// Velocidad meridional aguas arriba del borde, m/s: el caudal del rodete
    /// entre la sección de paso de la estación del corte.
    pub c1m: f64,
    /// Bloqueo del álabe, ≥ 1.
    pub tau1: f64,
    /// Ángulo del flujo β′1, rad.
    pub beta1_flow: f64,
    /// Ángulo del álabe β1B = β′1 + i1, rad.
    pub beta1b: f64,
    /// El ángulo que el álabe 2D actual tiene ahí.
    pub beta_2d: Blade2dAngle,
}

impl InletTriangle {
    /// Δβ = β 2D − β1B, rad. `None` si el álabe 2D no llega al corte o si
    /// la línea es axial en él.
    #[must_use]
    pub fn mismatch(&self) -> Option<f64> {
        match self.beta_2d {
            Blade2dAngle::Angle(beta) => Some(beta - self.beta1b),
            Blade2dAngle::NoBlade | Blade2dAngle::Axial => None,
        }
    }
}

/// El triángulo de entrada de cada línea, en el orden de `lines`.
///
/// `incidence_deg` es el i1 común, el mismo que usó el 1D. `impeller` es el
/// álabe 2D de hoy, del que sale la columna «β 2D».
///
/// # Errors
///
/// Los de la fórmula de β1B, los del espesor del álabe si el resultado no
/// trae Z, u2, nq y D2 válidos, y una línea cuya sección de paso en el corte
/// no sea positiva.
pub fn inlet_triangles(
    result: &DesignResult,
    impeller: &ImpellerGeometry,
    lines: &[Streamline],
    incidence_deg: f64,
) -> Result<Vec<InletTriangle>, DesignError> {
    let registry = Registry::all();
    let blade_count = result.selection.blade_count;
    let (_, thickness, _) =
        mechanics::blade_thicknesses(blade_count, result.u2.si, result.nq, result.d2.si)?;
    let omega = 2.0 * result.u1.si / result.d1.si;
    let blade_start = result.d1.si / 2.0;
    let incidence = incidence_deg.to_radians();

    lines
        .iter()
        .map(|line| {
            let section = line.leading_edge_area;
            if !(section.is_finite() && section > 0.0) {
                return Err(DesignError::Input(format!(
                    "la línea de corriente ψ = {:.2} no tiene sección de paso en su corte \
                     con el borde de ataque",
                    line.psi
                )));
            }
            let c1m = result.qth.si / (section / MM2_PER_M2);
            let r1 = line.leading_edge[1] / MM_PER_M;
            let u1 = omega * r1;
            // En el eje u1 = 0 y la fórmula divide por él: se da su límite, el
            // flujo axial (β′1 = 90°), sin bloqueo porque el diámetro es nulo.
            let (beta1b, tau1) = if u1 > 0.0 {
                leading_edge(
                    &registry,
                    incidence_deg,
                    c1m,
                    u1,
                    2.0 * r1,
                    thickness,
                    blade_count,
                )?
            } else {
                (std::f64::consts::FRAC_PI_2 + incidence, 1.0)
            };
            let beta_2d = if r1 < blade_start - BLADE_START_TOLERANCE {
                Blade2dAngle::NoBlade
            } else {
                angle_on_line(
                    plan_angle(&impeller.blade, r1),
                    line.sin_gamma_at_leading_edge(),
                )
            };
            Ok(InletTriangle {
                psi: line.psi,
                r1,
                u1,
                c1m,
                tau1,
                beta1_flow: beta1b - incidence,
                beta1b,
                beta_2d,
            })
        })
        .collect()
}

/// β de la línea media 2D en el radio `r`, m: interpolación lineal en r, con r
/// acotado al rango del álabe. Sin puntos, 90°.
fn plan_angle(blade: &[BladePoint], r: f64) -> f64 {
    let (Some(first), Some(last)) = (blade.first(), blade.last()) else {
        return std::f64::consts::FRAC_PI_2;
    };
    let r = r.clamp(first.r, last.r);
    for pair in blade.windows(2) {
        let span = pair[1].r - pair[0].r;
        if r <= pair[1].r && span > 0.0 {
            let fraction = ((r - pair[0].r) / span).clamp(0.0, 1.0);
            return pair[0].beta + (pair[1].beta - pair[0].beta) * fraction;
        }
    }
    last.beta
}

/// El β del plano radial visto sobre una línea de corriente con sen γ dado:
/// tan β = tan β_planta / sen γ. En un tramo axial no hay ángulo que dar.
fn angle_on_line(plan_beta: f64, sin_gamma: f64) -> Blade2dAngle {
    if sin_gamma < AXIAL_SIN_GAMMA {
        Blade2dAngle::Axial
    } else {
        Blade2dAngle::Angle((plan_beta.tan() / sin_gamma).atan())
    }
}
