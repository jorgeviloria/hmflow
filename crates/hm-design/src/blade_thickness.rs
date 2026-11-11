//! La curva de espesor e(x) del álabe, en cubo y en tapa.
//!
//! - **Abscisa.** La de β(x): x = 0 en el borde de ataque y x = 1 en la
//!   salida, fracción de la longitud meridional del álabe sobre cada línea de
//!   corriente ([`crate::blade_camber`]).
//! - **Forma.** Entre 2 y 8 nodos (x, e), el primero en x = 0 y el último en
//!   x = 1. Entre nodos, interpolación cúbica de Hermite monótona de Fritsch y
//!   Carlson (*Monotone Piecewise Cubic Interpolation*, SIAM J. Numer. Anal.,
//!   17(2), 1980, pp. 238-246): la curva no rebasa los nodos y un tramo entre
//!   dos nodos iguales queda plano. Ver [`slopes`] sobre lo que se comprobó.
//! - **Estado «Gülich».** No se guardan nodos: la curva se recalcula con el
//!   diseño con tres nodos, (0, e), (x_a, e) y (1, e/2), con e y e/2 de
//!   [`crate::mechanics::blade_thicknesses`] (Gülich, *Centrifugal Pumps*,
//!   4.ª ed., 2020, tabla 14.3). El adelgazamiento empieza
//!   [`GULICH_TAPER_THICKNESSES`]·e antes de la salida; ese 4 es de hmflow:
//!   Gülich fija e/2, no la longitud en que se alcanza.
//! - **Estado «editada».** Al mover, añadir o quitar un nodo, los nodos se
//!   guardan y dejan de seguir al diseño; [`BladeThickness::reset`] vuelve a
//!   «Gülich».
//! - **Cubo y tapa.** Dos curvas y un interruptor «cubo = tapa». Las líneas
//!   intermedias usan e_k(x) = (1 − ψ_k)·e_cubo(x) + ψ_k·e_tapa(x), la regla
//!   de [`crate::blade_setup::CamberBows::at`] (de hmflow).
//! - **Mínimo.** Ningún nodo baja de [`CAST_MIN_THICKNESS`] (3 mm; Gülich,
//!   §7.2.1, da «3–5 mm» como mínimo de fundición), salvo el de x = 1, cuyo
//!   mínimo es la mitad, coherente con el e/2 de Gülich ([`node_floor`]). Las
//!   dos cosas son decisiones de hmflow. Unidades: m.

use thiserror::Error;

use crate::DesignError;
use crate::DesignResult;
use crate::blade_camber::{BladeCamber, CamberLine};
use crate::mechanics::{CAST_MIN_THICKNESS, blade_thicknesses};

/// Cuántos nodos puede tener una curva: de 2 a 8. De hmflow.
pub const THICKNESS_NODES: (usize, usize) = (2, 8);

/// Separación mínima en x entre dos nodos. De hmflow.
pub const THICKNESS_NODE_GAP: f64 = 0.01;

/// Por debajo de este espesor (m) hay aviso, no error (Gülich, §7.2.1: «3–5
/// mm»; partirlo en límite y aviso es lectura de hmflow).
pub const THICKNESS_WARNING: f64 = 0.005;

/// Cuántos espesores antes de la salida empieza el adelgazamiento de
/// «Gülich». De hmflow.
pub const GULICH_TAPER_THICKNESSES: f64 = 4.0;

/// El adelgazamiento de «Gülich» no empieza antes de esta x. De hmflow.
pub const GULICH_TAPER_MIN_X: f64 = 0.5;

/// Un nodo de la curva: x de 0 a 1 y espesor e en m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThicknessNode {
    pub x: f64,
    pub e: f64,
}

/// La pared de una curva de espesor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThicknessWall {
    Hub,
    Shroud,
}

/// El mínimo de un nodo, m: [`CAST_MIN_THICKNESS`], y la mitad en x = 1.
#[must_use]
pub fn node_floor(x: f64) -> f64 {
    if x >= 1.0 {
        CAST_MIN_THICKNESS / 2.0
    } else {
        CAST_MIN_THICKNESS
    }
}

/// Por qué no se acepta una lista de nodos. Ninguno se arregla en silencio.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ThicknessError {
    #[error("la curva de espesor necesita entre 2 y 8 nodos y trae {0}")]
    NodeCount(usize),

    #[error("el primer nodo de la curva de espesor va en x = 0 y el último en x = 1")]
    Ends,

    #[error("el nodo {index} de la curva de espesor no está a la derecha del anterior")]
    NotIncreasing { index: usize },

    #[error(
        "el nodo en x = {} tiene {} mm de espesor, menos que su mínimo de {} mm",
        comma(*.x, 2),
        comma(*.e * 1000.0, 2),
        comma(node_floor(*.x) * 1000.0, 1)
    )]
    UnderFloor { x: f64, e: f64 },

    #[error("la curva de espesor tiene un valor que no es un número")]
    NotFinite,
}

/// Un número con coma decimal.
fn comma(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

/// Una curva e(x) validada: la pared de un álabe.
#[derive(Debug, Clone, PartialEq)]
pub struct ThicknessCurve {
    nodes: Vec<ThicknessNode>,
    /// Derivadas en los nodos (Fritsch y Carlson), calculadas al crearla.
    slopes: Vec<f64>,
}

impl ThicknessCurve {
    /// Valida los nodos: entre 2 y 8, x₀ = 0, x_último = 1, x estrictamente
    /// creciente, todo finito y e ≥ [`node_floor`].
    pub fn new(nodes: Vec<ThicknessNode>) -> Result<Self, ThicknessError> {
        if nodes.iter().any(|n| !n.x.is_finite() || !n.e.is_finite()) {
            return Err(ThicknessError::NotFinite);
        }
        if nodes.len() < THICKNESS_NODES.0 || nodes.len() > THICKNESS_NODES.1 {
            return Err(ThicknessError::NodeCount(nodes.len()));
        }
        if nodes[0].x != 0.0 || nodes[nodes.len() - 1].x != 1.0 {
            return Err(ThicknessError::Ends);
        }
        if let Some(index) = (1..nodes.len()).find(|&i| nodes[i].x <= nodes[i - 1].x) {
            return Err(ThicknessError::NotIncreasing { index });
        }
        if let Some(node) = nodes.iter().find(|n| n.e < node_floor(n.x)) {
            return Err(ThicknessError::UnderFloor {
                x: node.x,
                e: node.e,
            });
        }
        let slopes = slopes(&nodes);
        Ok(Self { nodes, slopes })
    }

    /// La curva de «Gülich»: (0, e), (x_a, e) y (1, e/2), con
    /// x_a = máx(0,5, 1 − 4·e/L). `mid_length` es L, la longitud verdadera de
    /// la línea media con ψ = 0,5; si no es finita o no es positiva, x_a = 0,5.
    /// No valida: `e` y `e_trailing` vienen de la mecánica.
    #[must_use]
    pub fn gulich(e: f64, e_trailing: f64, mid_length: f64) -> Self {
        let taper_start = if mid_length.is_finite() && mid_length > 0.0 {
            (1.0 - GULICH_TAPER_THICKNESSES * e / mid_length).max(GULICH_TAPER_MIN_X)
        } else {
            GULICH_TAPER_MIN_X
        };
        let nodes = vec![
            ThicknessNode { x: 0.0, e },
            ThicknessNode { x: taper_start, e },
            ThicknessNode {
                x: 1.0,
                e: e_trailing,
            },
        ];
        let slopes = slopes(&nodes);
        Self { nodes, slopes }
    }

    #[must_use]
    pub fn nodes(&self) -> &[ThicknessNode] {
        &self.nodes
    }

    /// El espesor en x, m. x se acota a [0, 1].
    #[must_use]
    pub fn at(&self, x: f64) -> f64 {
        let x = if x.is_finite() {
            x.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let last = self.nodes.len() - 2;
        let segment = self
            .nodes
            .partition_point(|node| node.x <= x)
            .saturating_sub(1)
            .min(last);
        let (a, b) = (self.nodes[segment], self.nodes[segment + 1]);
        let h = b.x - a.x;
        let (da, db) = (self.slopes[segment], self.slopes[segment + 1]);
        // Tramo plano: dos nodos iguales y derivadas nulas (Fritsch y
        // Carlson las anulan). Se devuelve el nodo tal cual, sin redondeo.
        if a.e == b.e && da == 0.0 && db == 0.0 {
            return a.e;
        }
        // Cúbica de Hermite en t = (x − x_a)/h.
        let t = (x - a.x) / h;
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * a.e + h10 * h * da + h01 * b.e + h11 * h * db
    }
}

/// Derivadas de la cúbica de Hermite monótona en cada nodo.
///
/// Con Δᵢ la pendiente del tramo i: la derivada de un extremo es la pendiente
/// de su tramo; la de un nodo interior, la media de las dos pendientes
/// vecinas, y 0 si tienen signos distintos o alguna es 0. Después, en cada
/// tramo con Δᵢ ≠ 0, α = dᵢ/Δᵢ y β = dᵢ₊₁/Δᵢ; si α² + β² > 9, las dos
/// derivadas se multiplican por 3/√(α² + β²); en un tramo con Δᵢ = 0 las dos
/// son 0.
///
/// **Qué se comprobó.** El artículo original (SIAM J. Numer. Anal., 1980) es
/// de pago y no se pudo consultar (acceso denegado, 403), así que estos pasos
/// **no están verificados contra él**: son los de la planta del plan, escritos
/// de memoria. Lo que sí hay son las pruebas de `tests/blade_thickness.rs`,
/// que miden lo que el método promete (la curva pasa por los nodos, no rebasa
/// el intervalo de cada tramo en 1001 muestras y los tramos entre nodos
/// iguales quedan planos), no su demostración.
fn slopes(nodes: &[ThicknessNode]) -> Vec<f64> {
    let count = nodes.len();
    let delta: Vec<f64> = nodes
        .windows(2)
        .map(|pair| (pair[1].e - pair[0].e) / (pair[1].x - pair[0].x))
        .collect();
    let mut slope = vec![0.0; count];
    slope[0] = delta[0];
    slope[count - 1] = delta[count - 2];
    for i in 1..count - 1 {
        let (before, after) = (delta[i - 1], delta[i]);
        let same_sign = (before > 0.0 && after > 0.0) || (before < 0.0 && after < 0.0);
        slope[i] = if same_sign {
            0.5 * (before + after)
        } else {
            0.0
        };
    }
    for i in 0..count - 1 {
        if delta[i] == 0.0 {
            slope[i] = 0.0;
            slope[i + 1] = 0.0;
        } else {
            let alpha = slope[i] / delta[i];
            let beta = slope[i + 1] / delta[i];
            let radius2 = alpha * alpha + beta * beta;
            if radius2 > 9.0 {
                let tau = 3.0 / radius2.sqrt();
                slope[i] = tau * alpha * delta[i];
                slope[i + 1] = tau * beta * delta[i];
            }
        }
    }
    slope
}

/// Los nodos que se guardan cuando la curva está «editada». Con
/// [`BladeThickness::same`], `hub` y `shroud` son iguales.
#[derive(Debug, Clone, PartialEq)]
pub struct EditedThickness {
    pub hub: Vec<ThicknessNode>,
    pub shroud: Vec<ThicknessNode>,
}

/// El espesor del álabe como lo guarda el diseño: el interruptor «cubo =
/// tapa» y, si la curva está editada, sus nodos. Por defecto, «Gülich».
#[derive(Debug, Clone, PartialEq)]
pub struct BladeThickness {
    /// «Cubo = tapa»: se edita y se guarda una sola curva.
    pub same: bool,
    /// `None` es «Gülich»: la curva sigue al diseño.
    pub edited: Option<EditedThickness>,
}

impl Default for BladeThickness {
    fn default() -> Self {
        Self {
            same: true,
            edited: None,
        }
    }
}

/// Las dos curvas de un álabe, de cubo y de tapa.
#[derive(Debug, Clone, PartialEq)]
pub struct ThicknessCurves {
    pub hub: ThicknessCurve,
    pub shroud: ThicknessCurve,
}

impl ThicknessCurves {
    /// El espesor de la línea con fracción de caudal `psi` en x, m:
    /// (1 − ψ)·cubo + ψ·tapa. ψ se acota a [0, 1]. De hmflow.
    #[must_use]
    pub fn at(&self, psi: f64, x: f64) -> f64 {
        let psi = psi.clamp(0.0, 1.0);
        (1.0 - psi) * self.hub.at(x) + psi * self.shroud.at(x)
    }
}

impl BladeThickness {
    /// Si los nodos están guardados («editada») y no siguen al diseño.
    #[must_use]
    pub fn is_edited(&self) -> bool {
        self.edited.is_some()
    }

    /// Las curvas: las de «Gülich» con `e`, `e_trailing` y `mid_length` (m), o
    /// las guardadas.
    pub fn curves(
        &self,
        e: f64,
        e_trailing: f64,
        mid_length: f64,
    ) -> Result<ThicknessCurves, ThicknessError> {
        match &self.edited {
            None => {
                let gulich = ThicknessCurve::gulich(e, e_trailing, mid_length);
                Ok(ThicknessCurves {
                    hub: gulich.clone(),
                    shroud: gulich,
                })
            }
            Some(edited) => Ok(ThicknessCurves {
                hub: ThicknessCurve::new(edited.hub.clone())?,
                shroud: ThicknessCurve::new(edited.shroud.clone())?,
            }),
        }
    }

    /// (e(0), e(1)) de la línea con ψ = 0,5 si la curva está editada; `None`
    /// en «Gülich». Es lo que el 1D usa para el bloqueo.
    #[must_use]
    pub fn edges(&self) -> Option<(f64, f64)> {
        let edited = self.edited.as_ref()?;
        let end = |nodes: &[ThicknessNode], last: bool| {
            if last { nodes.last() } else { nodes.first() }.map(|n| n.e)
        };
        let mid = |last: bool| {
            let hub = end(&edited.hub, last)?;
            let shroud = end(&edited.shroud, last)?;
            Some(0.5 * hub + 0.5 * shroud)
        };
        Some((mid(false)?, mid(true)?))
    }

    /// Los nodos de partida de una edición: los guardados, o los de `base`
    /// (las curvas que se ven) la primera vez.
    fn working(&self, base: &ThicknessCurves) -> EditedThickness {
        self.edited.clone().unwrap_or_else(|| EditedThickness {
            hub: base.hub.nodes().to_vec(),
            shroud: base.shroud.nodes().to_vec(),
        })
    }

    /// Con «cubo = tapa» las dos paredes se escriben a la vez y `wall` se
    /// ignora: la lista que se edita es siempre la del cubo.
    fn wall(&self, wall: ThicknessWall) -> ThicknessWall {
        if self.same { ThicknessWall::Hub } else { wall }
    }

    fn list(state: &mut EditedThickness, wall: ThicknessWall) -> &mut Vec<ThicknessNode> {
        match wall {
            ThicknessWall::Hub => &mut state.hub,
            ThicknessWall::Shroud => &mut state.shroud,
        }
    }

    fn with_state(&self, mut state: EditedThickness) -> Self {
        if self.same {
            state.shroud = state.hub.clone();
        }
        Self {
            same: self.same,
            edited: Some(state),
        }
    }

    /// Mueve el nodo `index` de `wall` a (x, e). Un nodo extremo solo cambia
    /// e; uno interior acota x entre sus vecinos a [`THICKNESS_NODE_GAP`]; e
    /// se acota por abajo a [`node_floor`]. Un valor no finito, o un índice
    /// que no existe, no cambia nada.
    #[must_use]
    pub fn with_node_moved(
        &self,
        base: &ThicknessCurves,
        wall: ThicknessWall,
        index: usize,
        x: f64,
        e: f64,
    ) -> Self {
        if !x.is_finite() || !e.is_finite() {
            return self.clone();
        }
        let mut state = self.working(base);
        let nodes = Self::list(&mut state, self.wall(wall));
        if index >= nodes.len() {
            return self.clone();
        }
        let last = nodes.len() - 1;
        let x = if index == 0 || index == last {
            nodes[index].x
        } else {
            let low = nodes[index - 1].x + THICKNESS_NODE_GAP;
            let high = nodes[index + 1].x - THICKNESS_NODE_GAP;
            if low <= high {
                x.clamp(low, high)
            } else {
                nodes[index].x
            }
        };
        nodes[index] = ThicknessNode {
            x,
            e: e.max(node_floor(x)),
        };
        self.with_state(state)
    }

    /// Añade un nodo en x sobre la curva de `wall`, con e = e(x) (subido a su
    /// mínimo si la curva pasa por debajo). `None` con 8 nodos, con x fuera de
    /// (0, 1) o a menos de [`THICKNESS_NODE_GAP`] de otro nodo.
    #[must_use]
    pub fn with_node_added(
        &self,
        base: &ThicknessCurves,
        wall: ThicknessWall,
        x: f64,
    ) -> Option<Self> {
        let mut state = self.working(base);
        let nodes = Self::list(&mut state, self.wall(wall));
        if !x.is_finite()
            || x <= 0.0
            || x >= 1.0
            || nodes.len() >= THICKNESS_NODES.1
            || nodes.iter().any(|n| (n.x - x).abs() < THICKNESS_NODE_GAP)
        {
            return None;
        }
        let curve = ThicknessCurve::new(nodes.clone()).ok()?;
        let at = nodes.partition_point(|n| n.x < x);
        nodes.insert(
            at,
            ThicknessNode {
                x,
                e: curve.at(x).max(node_floor(x)),
            },
        );
        Some(self.with_state(state))
    }

    /// Quita el nodo `index` de `wall`. `None` para un extremo, para un índice
    /// que no existe o si quedarían menos de 2 nodos.
    #[must_use]
    pub fn with_node_removed(
        &self,
        base: &ThicknessCurves,
        wall: ThicknessWall,
        index: usize,
    ) -> Option<Self> {
        let mut state = self.working(base);
        let nodes = Self::list(&mut state, self.wall(wall));
        if index == 0 || index + 1 >= nodes.len() || nodes.len() <= THICKNESS_NODES.0 {
            return None;
        }
        nodes.remove(index);
        Some(self.with_state(state))
    }

    /// Enciende o apaga «cubo = tapa». Apagarlo: la tapa arranca como copia
    /// del cubo (en «Gülich» no pasa a «editada»). Encenderlo en «editada»:
    /// la tapa toma los nodos del cubo y los suyos se pierden.
    #[must_use]
    pub fn with_same(&self, _base: &ThicknessCurves, same: bool) -> Self {
        let edited = self.edited.as_ref().map(|edited| {
            let mut state = edited.clone();
            // Con «cubo = tapa» las dos listas son iguales, así que apagarlo
            // ya deja la tapa como copia del cubo; encenderlo borra la tapa.
            if same {
                state.shroud = state.hub.clone();
            }
            state
        });
        Self { same, edited }
    }

    /// Vuelve a «Gülich» y conserva el interruptor.
    #[must_use]
    pub fn reset(&self) -> Self {
        Self {
            same: self.same,
            edited: None,
        }
    }
}

/// La línea con ψ más cercano a 0,5; la primera si hay empate.
#[must_use]
pub fn mid_line(camber: &BladeCamber) -> Option<&CamberLine> {
    camber
        .lines
        .iter()
        .min_by(|a, b| (a.psi - 0.5).abs().total_cmp(&(b.psi - 0.5).abs()))
}

/// Las curvas de espesor de un diseño: e y e/2 de
/// [`blade_thicknesses`] y L, la longitud verdadera de la línea media con
/// ψ = 0,5. Un error de los nodos guardados es `DesignError::Input` con su
/// texto.
pub fn thickness_curves_for_design(
    result: &DesignResult,
    camber: &BladeCamber,
    setup: &BladeThickness,
) -> Result<ThicknessCurves, DesignError> {
    let (_, e, e_trailing) = blade_thicknesses(
        result.selection.blade_count,
        result.u2.si,
        result.nq,
        result.d2.si,
    )?;
    let length = mid_line(camber).map_or(f64::NAN, |line| line.length);
    setup
        .curves(e, e_trailing, length)
        .map_err(|error| DesignError::Input(error.to_string()))
}
