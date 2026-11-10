//! La forma del álabe y en cuántas líneas de corriente se reparte el canal.
//!
//! De momento la forma no cambia ningún cálculo: las líneas de corriente y
//! los triángulos de entrada son los mismos para cualquiera. Solo se guarda y
//! se muestra, para que la línea media por línea de corriente (punto 2) y las
//! formas 2D que faltan (punto 4) encuentren el sitio hecho.
//!
//! La lista de formas es una idea de producto tomada de otros programas de
//! diseño de rodetes (`oracle/notas/cfturbo.md`, «Formas de álabe del
//! impulsor»); ningún algoritmo ni criterio sale de ahí.

/// Los números de líneas de corriente que se pueden elegir. Impares, para que
/// siempre haya una línea media. De hmflow.
pub const STREAMLINE_COUNTS: [u8; 4] = [3, 5, 7, 9];

/// Las líneas de corriente de un diseño que no dice otra cosa. De hmflow.
pub const DEFAULT_STREAMLINES: u8 = 5;

/// Punto del plan del álabe en que llegan las formas 3D.
const POINT_3D: u8 = 2;

/// Punto del plan del álabe en que llegan las 2D que faltan.
const POINT_2D: u8 = 4;

/// Si el álabe se traza sobre varias líneas de corriente o en planta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BladeFamily {
    /// Doble curvatura: una línea media por línea de corriente.
    ThreeD,
    /// Cilíndrico: una sola línea media, la misma del cubo a la tapa.
    TwoD,
}

impl BladeFamily {
    /// El rótulo del grupo en el selector.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ThreeD => "3D",
            Self::TwoD => "2D",
        }
    }
}

/// Las formas de álabe de un rodete radial de bomba.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BladeShape {
    /// Libre 3D.
    Free3D,
    /// Reglada 3D.
    Ruled3D,
    /// 2D libre: la línea media con β(r) de hoy.
    #[default]
    Free2D,
    /// 2D circular.
    Circular2D,
    /// 2D recta.
    Straight2D,
    /// 2D circular + libre.
    CircularFree2D,
    /// 2D espiral logarítmica + recta.
    LogSpiralStraight2D,
}

impl BladeShape {
    /// Todas, en el orden del selector: primero las 3D y luego las 2D.
    pub const ALL: [Self; 7] = [
        Self::Free3D,
        Self::Ruled3D,
        Self::Free2D,
        Self::Circular2D,
        Self::Straight2D,
        Self::CircularFree2D,
        Self::LogSpiralStraight2D,
    ];

    /// El nombre con que se guarda en el proyecto.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Free3D => "free_3d",
            Self::Ruled3D => "ruled_3d",
            Self::Free2D => "free_2d",
            Self::Circular2D => "circular_2d",
            Self::Straight2D => "straight_2d",
            Self::CircularFree2D => "circular_free_2d",
            Self::LogSpiralStraight2D => "log_spiral_straight_2d",
        }
    }

    /// La forma que se guarda con ese nombre, si existe.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shape| shape.id() == id)
    }

    /// El nombre dentro de su grupo ([`Self::family`]).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Free3D | Self::Free2D => "Libre",
            Self::Ruled3D => "Reglada",
            Self::Circular2D => "Circular",
            Self::Straight2D => "Recta",
            Self::CircularFree2D => "Circular + libre",
            Self::LogSpiralStraight2D => "Espiral logarítmica + recta",
        }
    }

    /// El grupo del selector.
    #[must_use]
    pub fn family(self) -> BladeFamily {
        match self {
            Self::Free3D | Self::Ruled3D => BladeFamily::ThreeD,
            Self::Free2D
            | Self::Circular2D
            | Self::Straight2D
            | Self::CircularFree2D
            | Self::LogSpiralStraight2D => BladeFamily::TwoD,
        }
    }

    /// El punto del plan del álabe en que llega; `None` si ya se puede elegir.
    #[must_use]
    pub fn arrives_in(self) -> Option<u8> {
        match self {
            Self::Free2D => None,
            Self::Free3D | Self::Ruled3D => Some(POINT_3D),
            Self::Circular2D
            | Self::Straight2D
            | Self::CircularFree2D
            | Self::LogSpiralStraight2D => Some(POINT_2D),
        }
    }

    /// Si ya se puede elegir. Hoy, solo la 2D libre.
    #[must_use]
    pub fn is_available(self) -> bool {
        self.arrives_in().is_none()
    }
}

/// Lo que el usuario elige del álabe antes de trazarlo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BladeSetup {
    pub shape: BladeShape,
    /// Uno de [`STREAMLINE_COUNTS`].
    pub streamlines: u8,
}

impl Default for BladeSetup {
    fn default() -> Self {
        Self {
            shape: BladeShape::default(),
            streamlines: DEFAULT_STREAMLINES,
        }
    }
}
