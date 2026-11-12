//! Los bordes del álabe: la nariz de la entrada y la forma de la salida.
//!
//! - **Tipo.** Uno por borde, igual en cubo y tapa: [`LeadingKind::Blunt`]
//!   (romo: el canto perpendicular de la curva e(x), sin nariz),
//!   [`LeadingKind::Ellipse`] y [`LeadingKind::Wedge`] (afilado). Por defecto,
//!   romo, y con él nada cambia.
//! - **Cotas por pared.** Cubo y tapa tienen las suyas ([`LeadingDims`]); la
//!   línea con fracción de caudal ψ interpola (1 − ψ)·cubo + ψ·tapa, la regla
//!   de [`crate::blade_thickness::ThicknessCurves::at`] (de hmflow). El
//!   interruptor «cubo = tapa» es el de [`crate::blade_thickness`]: este
//!   módulo no guarda otro y lo recibe como argumento.
//! - **Forma.** El espesor total t(s) de la nariz, con s sobre la línea
//!   media desde la punta y u = s/Lp:
//!   - elipse: t = e·√(1 − (1 − u)²), la semielipse de semiejes Lp y e/2
//!     (Gülich, *Centrifugal Pumps*, 4.ª ed., 2020, §7.2.1, fig. 7.3, p. 414);
//!   - afilado: t = tip + (e − tip)·u, la cuña de la misma figura; la punta
//!     `tip` y la longitud Lp son de hmflow (el libro no da valores);
//!   - asimetría a: la línea media del espesor se desplaza hacia la succión
//!     c = a·(t/2)·(1 − u)², h_s = t/2 + c y h_p = t/2 − c. Con a = 1 la cara
//!     de presión pasa por la línea media en la punta, como en la fig. 7.10e
//!     (§7.2.4, p. 425). **La fórmula con (1 − u)² es de hmflow**: el libro
//!     dibuja el caso, no da el reparto; (1 − u)² lo devuelve a la simetría
//!     con tangente continua en Lp.
//! - **cp,min** (Gülich, ec. 7.1a, p. 414): la caída de presión del borde de
//!   ataque a incidencia cero, [`cp_min`].
//! - **Límites** (de hmflow): Lp ≥ e/2 (el semicírculo; más corto sería más
//!   romo que él, y para eso está «romo»), Lp ≤ 0,3·L, punta entre 0 y e,
//!   asimetría entre 0 y 1 ([`LeadingDims::clamped`]).
//!
//! - **Salida.** Cuatro tipos ([`TrailingKind`]): recortada (la de la etapa 1),
//!   limada por succión, limada por presión y elipse. En el limado, la cara
//!   limada baja linealmente de e(x)/2 en el inicio del limado a `end − e(1)/2`
//!   en D2, de modo que el espesor total en D2 es `end` (Gülich, cap. 4.5.2,
//!   p. 207, y fig. 3.6, p. 92: sin perfilar, simétrica, «under-filing» por la
//!   succión y «over-filing» por la presión); la otra cara no cambia. En la
//!   elipse de salida, t = e(x)·√(1 − (1 − w)²), con w = v/longitud y v la
//!   distancia a D2: el espesor llega a 0 en D2. **La longitud del limado y de
//!   la elipse, y el espesor final, son de hmflow:** el libro no da cifras.
//!
//! Los bordes son forma: **no mueven el 1D**. Unidades: m.

use crate::blade_thickness::ThicknessWall;

/// Lp = 5·e al elegir un tipo, es decir e/Lp = 0,2 (Gülich, ec. 7.1a, da 0,155
/// de cp,min con ese cociente).
pub const NOSE_DEFAULT_RATIO: f64 = 5.0;

/// Lp ≤ 0,3·L, con L la longitud de la línea media. De hmflow.
pub const NOSE_MAX_FRACTION: f64 = 0.3;

/// Aviso cuando Lp pasa de 0,2·L. De hmflow.
pub const NOSE_LONG_FRACTION: f64 = 0.2;

/// Punta del afilado al elegirlo: 0,1·e. De hmflow.
pub const WEDGE_TIP_RATIO: f64 = 0.1;

/// Longitud del limado y de la elipse de salida al elegirlos: 4·e(1). De
/// hmflow.
pub const TAIL_DEFAULT_RATIO: f64 = 4.0;

/// La longitud de la salida no pasa de 0,3·L. De hmflow.
pub const TAIL_MAX_FRACTION: f64 = 0.3;

/// El 0,373 de la ec. 7.1a de Gülich (p. 414).
pub const CP_MIN_COEFFICIENT: f64 = 0.373;

/// Si `v` es un número finito mayor que cero.
fn is_positive(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

/// La forma del borde de ataque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeadingKind {
    /// Recto («blunt» de Gülich, el «romo» del spec): sin nariz, el canto de
    /// la curva e(x) tal cual, cortado perpendicular a la línea media. No es
    /// el semicírculo: ese es la elipse con Lp = e/2.
    #[default]
    Blunt,
    /// Semielipse de semiejes Lp y e/2.
    Ellipse,
    /// Afilado (cuña), con punta y asimetría.
    Wedge,
}

impl LeadingKind {
    /// Todos, en el orden del selector.
    pub const ALL: [Self; 3] = [Self::Blunt, Self::Ellipse, Self::Wedge];

    /// El nombre con que se guarda en el proyecto.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Blunt => "blunt",
            Self::Ellipse => "ellipse",
            Self::Wedge => "wedge",
        }
    }

    /// El tipo que se guarda con ese nombre, si existe.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }
}

/// Las cotas de una pared, en m. `tip` y `asymmetry` solo valen en
/// [`LeadingKind::Wedge`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LeadingDims {
    /// Longitud de la nariz sobre la línea media, desde la punta hasta el
    /// espesor completo.
    pub lp: f64,
    /// Espesor de la punta del afilado.
    pub tip: f64,
    /// Asimetría del afilado, de 0 (cuña simétrica) a 1 (la cara de presión
    /// sobre la línea media en la punta).
    pub asymmetry: f64,
}

impl LeadingDims {
    /// Las cotas al elegir `kind` con el espesor `e` de la pared: Lp = 5·e,
    /// punta 0,1·e (solo afilado) y asimetría 0. Romo no lleva cotas. De
    /// hmflow, salvo e/Lp = 0,2 de la elipse (Gülich, ec. 7.1a).
    #[must_use]
    pub fn default_for(kind: LeadingKind, e: f64) -> Self {
        match kind {
            LeadingKind::Blunt => Self::default(),
            LeadingKind::Ellipse => Self {
                lp: NOSE_DEFAULT_RATIO * e,
                tip: 0.0,
                asymmetry: 0.0,
            },
            LeadingKind::Wedge => Self {
                lp: NOSE_DEFAULT_RATIO * e,
                tip: WEDGE_TIP_RATIO * e,
                asymmetry: 0.0,
            },
        }
    }

    /// Las cotas dentro de los límites: Lp entre e/2 y 0,3·`length`, punta
    /// entre 0 y e, asimetría entre 0 y 1. Si algo no es finito, `None`:
    /// ningún valor se inventa. Con una línea tan corta que 0,3·L < e/2 gana
    /// el mínimo e/2 (el semicírculo): la nariz que no cabe en la línea la
    /// avisa quien la construye, no se acorta aquí en silencio. De hmflow.
    #[must_use]
    pub fn clamped(self, e: f64, length: f64) -> Option<Self> {
        let finite = [self.lp, self.tip, self.asymmetry, e, length]
            .iter()
            .all(|v| v.is_finite());
        if !finite {
            return None;
        }
        Some(Self {
            lp: self.lp.min(NOSE_MAX_FRACTION * length).max(e / 2.0),
            tip: self.tip.clamp(0.0, e.max(0.0)),
            asymmetry: self.asymmetry.clamp(0.0, 1.0),
        })
    }
}

/// La forma del borde de salida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrailingKind {
    /// Recortada en D2: el canto de la etapa 1.
    #[default]
    Cut,
    /// Limada por la cara de succión.
    FiledSuction,
    /// Limada por la cara de presión.
    FiledPressure,
    /// Semielipse: el espesor llega a 0 en D2.
    Ellipse,
}

impl TrailingKind {
    /// Todos, en el orden del selector.
    pub const ALL: [Self; 4] = [
        Self::Cut,
        Self::FiledSuction,
        Self::FiledPressure,
        Self::Ellipse,
    ];

    /// El nombre con que se guarda en el proyecto.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Cut => "cut",
            Self::FiledSuction => "filed_suction",
            Self::FiledPressure => "filed_pressure",
            Self::Ellipse => "ellipse",
        }
    }

    /// El tipo que se guarda con ese nombre, si existe.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }
}

/// Las cotas de la salida de una pared, en m. `length` se mide sobre la línea
/// media hasta D2; `end` (el espesor total en D2) solo vale en el limado.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrailingDims {
    pub length: f64,
    pub end: f64,
}

impl TrailingDims {
    /// Las cotas al elegir `kind` con el espesor de salida `e1` de la pared:
    /// longitud 4·e(1) y, en el limado, espesor final e(1)/2. La recortada no
    /// lleva cotas y la elipse no lleva `end`. De hmflow.
    #[must_use]
    pub fn default_for(kind: TrailingKind, e1: f64) -> Self {
        match kind {
            TrailingKind::Cut => Self::default(),
            TrailingKind::FiledSuction | TrailingKind::FiledPressure => Self {
                length: TAIL_DEFAULT_RATIO * e1,
                end: e1 / 2.0,
            },
            TrailingKind::Ellipse => Self {
                length: TAIL_DEFAULT_RATIO * e1,
                end: 0.0,
            },
        }
    }

    /// Las cotas dentro de los límites: longitud entre e(1)/2 (de hmflow) y
    /// 0,3·`length_total`, y espesor final entre `floor` (el mínimo de
    /// fundición de x = 1, [`crate::blade_thickness::node_floor`]) y e(1) (que
    /// gana si e(1) está bajo el suelo). Si algo no es finito, `None`. De
    /// hmflow.
    #[must_use]
    pub fn clamped(self, e1: f64, length_total: f64, floor: f64) -> Option<Self> {
        let finite = [self.length, self.end, e1, length_total, floor]
            .iter()
            .all(|v| v.is_finite());
        if !finite {
            return None;
        }
        Some(Self {
            length: self
                .length
                .min(TAIL_MAX_FRACTION * length_total)
                .max(e1 / 2.0),
            // Limar no engorda la salida: con e(1) bajo el suelo, e(1) gana.
            end: self.end.max(floor).min(e1),
        })
    }
}

/// El perfil de la salida de una línea de corriente (ψ ya interpolado), listo
/// para evaluar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TailProfile {
    pub kind: TrailingKind,
    pub dims: TrailingDims,
}

impl TailProfile {
    /// (h_presión, h_succión) en m a la distancia `v` (m) de D2 hacia dentro
    /// sobre la línea media, con `e` = e(x) en ese punto y `e1` = e(1). Fuera
    /// de [0, longitud], con recortada, o con una longitud que no es positiva:
    /// (e/2, e/2).
    #[must_use]
    pub fn half_thicknesses(&self, v: f64, e: f64, e1: f64) -> (f64, f64) {
        let body = (e / 2.0, e / 2.0);
        let length = self.dims.length;
        if self.kind == TrailingKind::Cut || !is_positive(length) || !(0.0..=length).contains(&v) {
            return body;
        }
        let w = v / length;
        // El limado: la cara limada pasa linealmente de e(x)/2 (w = 1) a
        // `end − e(1)/2` (w = 0) (Gülich, 4.5.2, fig. 3.6; la rampa es de
        // hmflow).
        let filed = |face: f64| {
            let at_exit = self.dims.end - e1 / 2.0;
            at_exit + w * (face - at_exit)
        };
        match self.kind {
            TrailingKind::Cut => body,
            TrailingKind::FiledSuction => (e / 2.0, filed(e / 2.0)),
            TrailingKind::FiledPressure => (filed(e / 2.0), e / 2.0),
            TrailingKind::Ellipse => {
                let half = 0.5 * e * (1.0 - (1.0 - w) * (1.0 - w)).max(0.0).sqrt();
                (half, half)
            }
        }
    }
}

/// Los bordes de un álabe: la entrada y la salida.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BladeEdges {
    /// El tipo de la salida: uno solo, vale para cubo y tapa.
    pub trailing: TrailingKind,
    /// Las cotas de la salida en el cubo.
    pub trailing_hub: TrailingDims,
    /// Las cotas de la salida en la tapa. Con «cubo = tapa» no se leen.
    pub trailing_shroud: TrailingDims,
    /// El tipo de la entrada: uno solo, vale para cubo y tapa.
    pub leading: LeadingKind,
    /// Las cotas de la entrada en el cubo.
    pub leading_hub: LeadingDims,
    /// Las cotas de la entrada en la tapa. Con «cubo = tapa» no se leen.
    pub leading_shroud: LeadingDims,
}

/// El perfil de la nariz de una línea de corriente (ψ ya interpolado), listo
/// para evaluar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoseProfile {
    pub kind: LeadingKind,
    pub dims: LeadingDims,
}

impl NoseProfile {
    /// (h_presión, h_succión) en m a la distancia `s` (m) de la punta, con
    /// `e` = e(x) en ese punto. Fuera de [0, Lp], con romo, o con Lp que no
    /// es positiva: (e/2, e/2), el cuerpo de la etapa 1. La elipse es
    /// simétrica; la asimetría solo cuenta en el afilado, se acota a [0, 1]
    /// y mantiene h_p ≥ 0 (reparto de hmflow, ver el módulo).
    #[must_use]
    pub fn half_thicknesses(&self, s: f64, e: f64) -> (f64, f64) {
        let body = (e / 2.0, e / 2.0);
        let lp = self.dims.lp;
        if self.kind == LeadingKind::Blunt || !is_positive(lp) || !(0.0..=lp).contains(&s) {
            return body;
        }
        let u = s / lp;
        let total = match self.kind {
            LeadingKind::Blunt => return body,
            // Gülich §7.2.1, fig. 7.3. El máx protege la raíz del redondeo en
            // u = 1.
            LeadingKind::Ellipse => e * (1.0 - (1.0 - u) * (1.0 - u)).max(0.0).sqrt(),
            LeadingKind::Wedge => self.dims.tip + (e - self.dims.tip) * u,
        };
        let half = total / 2.0;
        if self.kind == LeadingKind::Wedge {
            // Asimetría de hmflow (fig. 7.10e de Gülich solo dibuja el caso).
            let a = self.dims.asymmetry.clamp(0.0, 1.0);
            let shift = a * half * (1.0 - u) * (1.0 - u);
            (half - shift, half + shift)
        } else {
            (half, half)
        }
    }

    /// e/Lp, el cociente de la ec. 7.1a; `None` con romo o con Lp que no es
    /// positiva.
    #[must_use]
    pub fn ratio(&self, e: f64) -> Option<f64> {
        if self.kind == LeadingKind::Blunt || !is_positive(self.dims.lp) {
            return None;
        }
        Some(e / self.dims.lp)
    }
}

/// cp,min = 0,373·(e/Lp)·(2 + 0,373·e/Lp): la depresión máxima del borde de
/// ataque a incidencia cero, Δp = cp,min·(ρ/2)·w1² (Gülich, ec. 7.1a, p. 414).
/// Da 2,05 con el semicírculo (e/Lp = 2) y 0,155 con e/Lp = 0,2. Gülich avisa
/// de que no se transfiere tal cual al rodete: es una lectura orientativa.
#[must_use]
pub fn cp_min(e_over_lp: f64) -> f64 {
    CP_MIN_COEFFICIENT * e_over_lp * (2.0 + CP_MIN_COEFFICIENT * e_over_lp)
}

impl BladeEdges {
    /// El perfil de la nariz en la línea con fracción de caudal `psi`:
    /// (1 − ψ)·cubo + ψ·tapa en Lp, punta y asimetría (de hmflow). ψ se acota
    /// a [0, 1]. Con `same` («cubo = tapa»), las cotas del cubo.
    #[must_use]
    pub fn profile_at(&self, psi: f64, same: bool) -> NoseProfile {
        let dims = if same {
            self.leading_hub
        } else {
            let psi = if psi.is_finite() {
                psi.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (hub, shroud) = (self.leading_hub, self.leading_shroud);
            let blend = |h: f64, t: f64| (1.0 - psi) * h + psi * t;
            LeadingDims {
                lp: blend(hub.lp, shroud.lp),
                tip: blend(hub.tip, shroud.tip),
                asymmetry: blend(hub.asymmetry, shroud.asymmetry),
            }
        };
        NoseProfile {
            kind: self.leading,
            dims,
        }
    }

    /// Cambia el tipo de la entrada. Las cotas de las dos paredes arrancan con
    /// [`LeadingDims::default_for`], cada una con su e(0): `e_hub` y
    /// `e_shroud`. Desde ahí se guardan y dejan de seguir al diseño, como un
    /// nodo editado; las del tipo anterior no se conservan aquí, pero el
    /// `BladeSetup` anterior las guarda para «Deshacer».
    #[must_use]
    pub fn with_leading_kind(&self, kind: LeadingKind, e_hub: f64, e_shroud: f64) -> Self {
        Self {
            leading: kind,
            leading_hub: LeadingDims::default_for(kind, e_hub),
            leading_shroud: LeadingDims::default_for(kind, e_shroud),
            ..self.clone()
        }
    }

    /// Escribe las cotas de `wall` y deja la otra pared como estaba. Con
    /// `same` («cubo = tapa») se escriben las dos y `wall` se ignora, como
    /// en [`crate::blade_thickness::BladeThickness`].
    #[must_use]
    pub fn with_leading_dims(&self, wall: ThicknessWall, dims: LeadingDims, same: bool) -> Self {
        let mut edges = self.clone();
        match (same, wall) {
            (true, _) => {
                edges.leading_hub = dims;
                edges.leading_shroud = dims;
            }
            (false, ThicknessWall::Hub) => edges.leading_hub = dims,
            (false, ThicknessWall::Shroud) => edges.leading_shroud = dims,
        }
        edges
    }

    /// Enciende o apaga «cubo = tapa». Apagarlo: la tapa arranca como copia
    /// del cubo. Encenderlo: manda el cubo y las cotas de la tapa se pierden.
    /// Las dos cosas dejan las dos paredes iguales, como en
    /// [`crate::blade_thickness::BladeThickness::with_same`].
    #[must_use]
    pub fn with_same(&self, _same: bool) -> Self {
        Self {
            leading_shroud: self.leading_hub,
            trailing_shroud: self.trailing_hub,
            ..self.clone()
        }
    }

    /// El perfil de la salida en la línea con fracción de caudal `psi`:
    /// (1 − ψ)·cubo + ψ·tapa en la longitud y en el espesor final (de
    /// hmflow). ψ se acota a [0, 1]. Con `same`, las cotas del cubo.
    #[must_use]
    pub fn tail_at(&self, psi: f64, same: bool) -> TailProfile {
        let dims = if same {
            self.trailing_hub
        } else {
            let psi = if psi.is_finite() {
                psi.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (hub, shroud) = (self.trailing_hub, self.trailing_shroud);
            let blend = |h: f64, t: f64| (1.0 - psi) * h + psi * t;
            TrailingDims {
                length: blend(hub.length, shroud.length),
                end: blend(hub.end, shroud.end),
            }
        };
        TailProfile {
            kind: self.trailing,
            dims,
        }
    }

    /// Cambia el tipo de la salida; la entrada no se toca. Las cotas de las
    /// dos paredes arrancan con [`TrailingDims::default_for`], cada una con su
    /// e(1): `e1_hub` y `e1_shroud`.
    #[must_use]
    pub fn with_trailing_kind(&self, kind: TrailingKind, e1_hub: f64, e1_shroud: f64) -> Self {
        Self {
            trailing: kind,
            trailing_hub: TrailingDims::default_for(kind, e1_hub),
            trailing_shroud: TrailingDims::default_for(kind, e1_shroud),
            ..self.clone()
        }
    }

    /// Escribe las cotas de salida de `wall` y deja la otra pared como estaba.
    /// Con `same` se escriben las dos y `wall` se ignora.
    #[must_use]
    pub fn with_trailing_dims(&self, wall: ThicknessWall, dims: TrailingDims, same: bool) -> Self {
        let mut edges = self.clone();
        match (same, wall) {
            (true, _) => {
                edges.trailing_hub = dims;
                edges.trailing_shroud = dims;
            }
            (false, ThicknessWall::Hub) => edges.trailing_hub = dims,
            (false, ThicknessWall::Shroud) => edges.trailing_shroud = dims,
        }
        edges
    }
}
