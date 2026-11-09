//! Contorno mecánico de las tapas, editado sobre la curva: cortar, pasar un
//! tramo a recta y mover sus puntos. Es geometría de fabricación encima de la
//! Bézier hidráulica, que no cambia.
//!
//! Cada tapa es una cadena de nodos (el arranque, los cortes y el final) con
//! un tramo entre cada dos: curva (la Bézier entre los `t` de sus nodos) o
//! recta. Un nodo está sobre la Bézier, en su `t`, salvo que todos sus tramos
//! vecinos sean rectas: entonces puede quedar libre, en cualquier (z, r). Un
//! nodo que toca una curva se desliza sobre ella, para que el contorno no se
//! rompa.
//!
//! Un tramo también puede ser un arco de radio dado entre sus dos nodos;
//! para los nodos cuenta como una recta (puede haber nodos libres a su lado).
//!
//! Una esquina con una recta al lado se puede redondear con un radio: el
//! nodo sigue siendo el vértice teórico y el arco tangente recorta los dos
//! tramos al dibujar.
//!
//! Se guarda en `[meridian.segments]` del `.hmf`, así que entra en deshacer
//! como lo demás de la pestaña.

use eframe::egui;
use egui::Color32;
use egui_plot::{PlotPoint, Points};
use hm_design::meridian::{MeridionalModel, Wall};
use hm_project::{Meridian, MeridianWallSegments};

use crate::curves::line;
use crate::meridian_arc::{self, Arc, Tangent};
use crate::meridian_constraints::Constraints;
use crate::meridian_fillet::{self, Chamfer, Fillet, Side};

/// Distancia en píxeles para tocar una tapa o un punto con el ratón.
const TOUCH_PIXELS: f32 = 12.0;

/// Distancia en píxeles a la que un punto libre se pega al eje o a la z o la
/// r de otro punto.
const SNAP_PIXELS: f32 = 6.0;

/// Hueco mínimo, en `t`, entre dos nodos.
const MIN_GAP: f64 = 0.005;
/// Largo mínimo, en mm, de una recta que prolonga la cadena.
const MIN_LENGTH_MM: f64 = 0.01;

/// Puntos con que se dibuja cada tramo curvo.
const SEGMENT_POINTS: usize = 80;

/// Puntos con que se dibuja el arco de un radio.
const ARC_POINTS: usize = 40;

/// Color de un radio que no cabe.
const REFUSED: Color32 = Color32::from_rgb(210, 40, 40);

/// Color de una cota que se escribe con un clic, como las del paso C1.
const DIMENSION: Color32 = Color32::from_rgb(20, 90, 200);

/// Color del tramo seleccionado y de las guías de ajuste.
const SELECTED: Color32 = Color32::from_rgb(230, 120, 0);

/// Una punta de la cadena de nodos de una tapa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum End {
    /// La del nodo 0, antes de P0.
    Start,
    /// La del último nodo, después de P4.
    Finish,
}

/// Qué es un tramo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Piece {
    /// La Bézier entre los `t` de sus nodos.
    Curve,
    /// La recta entre sus nodos.
    Line,
    /// El arco menor de radio |R| mm entre sus nodos; el signo, como en
    /// [`Arc`]: positivo, curvado a la izquierda de la cuerda.
    Arc(f64),
}

impl Piece {
    /// Recta o arco: sus extremos no tienen por qué estar en la Bézier.
    fn rigid(self) -> bool {
        self != Self::Curve
    }
}

/// De qué lado sale tangente un arco a su vecino.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tangency {
    /// Al tramo anterior, en su nodo de arranque; el final queda sobre el
    /// círculo.
    Start,
    /// Al tramo siguiente, en su nodo final; el arranque queda sobre el
    /// círculo.
    End,
}

/// Los nodos y los tramos de una tapa. Hay `cuts.len() + 2` nodos (el 0 es el
/// arranque y el último el final) y `cuts.len() + 1` tramos; el tramo `i` va
/// del nodo `i` al `i + 1`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct WallSegments {
    /// Nodos de la prolongación antes de P0: rectas o arcos, siempre libres.
    head: usize,
    /// Nodos de la prolongación después de P4.
    tail: usize,
    /// Si una recta cierra la cadena, del último nodo al nodo 0. Se dibuja,
    /// pero no es un tramo de `pieces`.
    closed: bool,
    /// El `t` de cada corte sobre la Bézier, creciente.
    cuts: Vec<f64>,
    /// Qué es cada tramo.
    pieces: Vec<Piece>,
    /// La posición (z, r) en mm de cada nodo libre; `None`, sobre la Bézier.
    free: Vec<Option<[f64; 2]>>,
    /// El radio en mm de cada nodo; `None`, esquina viva.
    radius: Vec<Option<f64>>,
    /// El chaflán en mm de cada nodo entre dos rectas; `None`, sin chaflán.
    /// Un nodo lleva radio o chaflán.
    chamfer: Vec<Option<f64>>,
    /// Si cada tramo (un arco) sale tangente a un vecino.
    tangent: Vec<Option<Tangency>>,
}

/// Una cota de un punto.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Dimension {
    /// La z, desde el plano de entrada (z = 0).
    Z,
    /// El diámetro, desde el eje.
    Diameter,
    /// El radio de un arco; el índice que la acompaña es el del tramo.
    Arc,
}

/// Cómo queda la esquina de un nodo.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Corner {
    /// Sin radio, o un nodo que no es esquina.
    Sharp,
    /// Redondeada.
    Round(Fillet),
    /// Cortada por un chaflán.
    Chamfer(Chamfer),
    /// Con un chaflán que no cabe: se dibuja viva.
    TooBigChamfer(f64),
    /// Con un radio que no cabe: se dibuja viva.
    TooBig(f64),
}

impl Default for WallSegments {
    fn default() -> Self {
        Self {
            head: 0,
            tail: 0,
            closed: false,
            cuts: Vec::new(),
            pieces: vec![Piece::Curve],
            free: vec![None, None],
            radius: vec![None, None],
            chamfer: vec![None, None],
            tangent: vec![None],
        }
    }
}

impl WallSegments {
    /// Lee los textos del `.hmf`; `None` si no se entienden (la validación
    /// del proyecto ya lo avisa con HM-PROJ-031).
    pub(super) fn from_texts(texts: &MeridianWallSegments) -> Option<Self> {
        let mut cuts = Vec::with_capacity(texts.cuts.len());
        for text in &texts.cuts {
            let t: f64 = text.trim().parse().ok()?;
            if !(t > cuts.last().copied().unwrap_or(0.0) && t < 1.0) {
                return None;
            }
            cuts.push(t);
        }
        let nodes = texts.node_count();
        let count = nodes - 1;
        if !(texts.pieces.is_empty() || texts.pieces.len() == count)
            || !(texts.arcs.is_empty() || texts.arcs.len() == count)
            || !(texts.tangents.is_empty() || texts.tangents.len() == count)
        {
            return None;
        }
        let tangent = (0..count)
            .map(
                |i| match texts.tangents.get(i).map_or("", |text| text.trim()) {
                    "" => Some(None),
                    "start" => Some(Some(Tangency::Start)),
                    "end" => Some(Some(Tangency::End)),
                    _ => None,
                },
            )
            .collect::<Option<Vec<_>>>()?;
        let pieces = (0..count)
            .map(|i| {
                let arc = texts.arcs.get(i).map_or("", |text| text.trim());
                match (
                    texts.pieces.get(i).map_or("curve", |piece| piece.trim()),
                    arc,
                ) {
                    ("curve", "") => Some(Piece::Curve),
                    ("line", "") => Some(Piece::Line),
                    ("arc", text) => length_mm(text).filter(|r| *r != 0.0).map(Piece::Arc),
                    _ => None,
                }
            })
            .collect::<Option<Vec<Piece>>>()?;
        for (name, text) in [("head", &texts.head), ("tail", &texts.tail)] {
            if !text.trim().is_empty() && text.trim().parse::<usize>().is_err() {
                eprintln!("segmentos: {name} no es un entero: {text}");
                return None;
            }
        }
        let (head, tail) = (texts.head_count(), texts.tail_count());
        let free = if texts.nodes.is_empty() {
            vec![None; nodes]
        } else if texts.nodes.len() == nodes {
            texts
                .nodes
                .iter()
                .map(|text| {
                    if text.trim().is_empty() {
                        Some(None)
                    } else {
                        node_position(text).map(Some)
                    }
                })
                .collect::<Option<Vec<_>>>()?
        } else {
            return None;
        };
        let radius = if texts.radii.is_empty() {
            vec![None; nodes]
        } else if texts.radii.len() == nodes {
            texts
                .radii
                .iter()
                .map(|text| {
                    if text.trim().is_empty() {
                        Some(None)
                    } else {
                        length_mm(text).filter(|r| *r > 0.0).map(Some)
                    }
                })
                .collect::<Option<Vec<_>>>()?
        } else {
            return None;
        };
        let chamfer = if texts.chamfers.is_empty() {
            vec![None; nodes]
        } else if texts.chamfers.len() == nodes {
            texts
                .chamfers
                .iter()
                .map(|text| {
                    if text.trim().is_empty() {
                        Some(None)
                    } else {
                        length_mm(text).filter(|d| *d > 0.0).map(Some)
                    }
                })
                .collect::<Option<Vec<_>>>()?
        } else {
            return None;
        };
        // Un nodo de la prolongación necesita su posición.
        if (0..nodes).any(|i| (i < head || i >= nodes - tail) && free[i].is_none()) {
            return None;
        }
        let mut state = Self {
            head,
            tail,
            closed: texts.is_closed(),
            cuts,
            pieces,
            free,
            radius,
            chamfer,
            tangent,
        };
        state.settle();
        Some(state)
    }

    /// Los textos para el `.hmf`: sin rectas, `pieces` va vacío; sin nodos
    /// libres, `nodes` también.
    fn to_texts(&self) -> MeridianWallSegments {
        let cuts = self.cuts.iter().map(|t| trimmed(*t, 6)).collect();
        let pieces = if self.has_rigid() {
            self.pieces
                .iter()
                .map(|piece| match piece {
                    Piece::Curve => "curve",
                    Piece::Line => "line",
                    Piece::Arc(_) => "arc",
                })
                .map(str::to_string)
                .collect()
        } else {
            Vec::new()
        };
        let arcs = if self
            .pieces
            .iter()
            .any(|piece| matches!(piece, Piece::Arc(_)))
        {
            self.pieces
                .iter()
                .map(|piece| match piece {
                    Piece::Arc(r) => format!("{} mm", trimmed(*r, 4)),
                    _ => String::new(),
                })
                .collect()
        } else {
            Vec::new()
        };
        let nodes = if self.free.iter().any(Option::is_some) {
            self.free
                .iter()
                .map(|free| {
                    free.map_or_else(String::new, |[z, r]| {
                        format!("{} mm, {} mm", trimmed(z, 4), trimmed(r, 4))
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        let radii = if self.radius.iter().any(Option::is_some) {
            self.radius
                .iter()
                .map(|radius| radius.map_or_else(String::new, |r| format!("{} mm", trimmed(r, 4))))
                .collect()
        } else {
            Vec::new()
        };
        let chamfers = if self.chamfer.iter().any(Option::is_some) {
            self.chamfer
                .iter()
                .map(|d| d.map_or_else(String::new, |d| format!("{} mm", trimmed(d, 4))))
                .collect()
        } else {
            Vec::new()
        };
        let tangents = if self.tangent.iter().any(Option::is_some) {
            self.tangent
                .iter()
                .map(|tangent| match tangent {
                    Some(Tangency::Start) => "start",
                    Some(Tangency::End) => "end",
                    None => "",
                })
                .map(str::to_string)
                .collect()
        } else {
            Vec::new()
        };
        MeridianWallSegments {
            head: if self.head > 0 {
                self.head.to_string()
            } else {
                String::new()
            },
            tail: if self.tail > 0 {
                self.tail.to_string()
            } else {
                String::new()
            },
            closed: if self.closed {
                "true".to_string()
            } else {
                String::new()
            },
            cuts,
            pieces,
            nodes,
            radii,
            chamfers,
            arcs,
            tangents,
        }
    }

    fn is_plain(&self) -> bool {
        self.cuts.is_empty() && self.head == 0 && self.tail == 0
    }

    /// Si algún tramo es recta o arco.
    fn has_rigid(&self) -> bool {
        self.pieces.iter().any(|piece| piece.rigid())
    }

    pub(super) fn node_count(&self) -> usize {
        self.head + self.cuts.len() + 2 + self.tail
    }

    /// El nodo P0, donde empieza la Bézier.
    fn p0(&self) -> usize {
        self.head
    }

    /// El nodo P4, donde acaba la Bézier.
    fn p4(&self) -> usize {
        self.head + self.cuts.len() + 1
    }

    /// Si el nodo es de la prolongación, fuera de la Bézier.
    pub(super) fn is_extension(&self, index: usize) -> bool {
        index < self.p0() || index > self.p4()
    }

    /// El lugar en `cuts` del nodo `index`, si es un corte sobre la Bézier.
    fn cut_slot(&self, index: usize) -> Option<usize> {
        (self.p0() < index && index < self.p4()).then(|| index - self.head - 1)
    }

    /// Por qué no se puede proponer el contorno cerrado (cubo al eje), o
    /// `Ok`: sin prolongación ni cierre, con P0 y P4 sobre la curva, con el
    /// espesor de pared del diseño (mm) y con algo que prolongar hasta el eje.
    pub(super) fn can_propose_closed(
        &self,
        points: &[[f64; 2]; 5],
        thickness: Option<f64>,
    ) -> Result<(), &'static str> {
        if self.head + self.tail > 0 || self.closed {
            return Err("ya tiene prolongación");
        }
        if self.free[self.p0()].is_some() || self.free[self.p4()].is_some() {
            return Err("P0 y P4 deben seguir sobre la curva");
        }
        let Some(wall) = thickness.filter(|e| e.is_finite() && *e >= 0.0) else {
            return Err("falta el espesor de pared del diseño");
        };
        let (start, finish) = (points[0], points[4]);
        let (start_out, finish_out) = (start[1] > MIN_LENGTH_MM, finish[1] > MIN_LENGTH_MM);
        // Si P4 está en el eje no hay cara trasera: la cadena trasera no se crea.
        let back = finish[0] + if finish_out { wall } else { 0.0 };
        let apart = distance([start[0], 0.0], [back, 0.0]) >= MIN_LENGTH_MM;
        if !(start_out || finish_out) || (start_out && finish_out && !apart) {
            return Err("no hay nada que cerrar");
        }
        Ok(())
    }

    /// Cierra el cubo al eje con rectas: de P0 al eje; de P4 hacia atrás el
    /// espesor de pared `thickness` (mm) y luego al eje; y una recta entre las
    /// dos puntas. Con espesor 0 baja en vertical desde P4.
    pub(super) fn propose_closed(
        &mut self,
        points: &[[f64; 2]; 5],
        thickness: Option<f64>,
    ) -> Result<(), &'static str> {
        self.can_propose_closed(points, thickness)?;
        let wall = thickness.unwrap_or(0.0);
        let (start, finish) = (points[0], points[4]);
        if start[1] > MIN_LENGTH_MM {
            self.extend(points, End::Start, [start[0], 0.0]);
        }
        if finish[1] > MIN_LENGTH_MM {
            if wall >= MIN_LENGTH_MM {
                self.extend(points, End::Finish, [finish[0] + wall, finish[1]]);
            }
            self.extend(points, End::Finish, [finish[0] + wall, 0.0]);
        }
        self.closed = self.head + self.tail > 0;
        Ok(())
    }

    /// Por qué no se puede proponer el contorno cerrado de la tapa, o `Ok`:
    /// sin prolongación ni cierre, con P0 y P4 sobre la curva y con un espesor
    /// de pared del diseño (mm) que dé tramos de verdad.
    pub(super) fn can_propose_shroud(&self, thickness: Option<f64>) -> Result<(), &'static str> {
        if self.head + self.tail > 0 || self.closed {
            return Err("ya tiene prolongación");
        }
        if self.free[self.p0()].is_some() || self.free[self.p4()].is_some() {
            return Err("P0 y P4 deben seguir sobre la curva");
        }
        match thickness {
            Some(e) if e.is_finite() && e >= MIN_LENGTH_MM => Ok(()),
            Some(e) if e.is_finite() && e >= 0.0 => Err("el espesor de pared es demasiado fino"),
            _ => Err("falta el espesor de pared del diseño"),
        }
    }

    /// Cierra la tapa por fuera del hidráulico con un offset: desde P4, rectas
    /// por [`SHROUD_OFFSET_STEPS`] puntos de la curva paralela a distancia
    /// `thickness` (mm), hasta el de P0, y una recta de cierre hasta P0.
    pub(super) fn propose_shroud(
        &mut self,
        points: &[[f64; 2]; 5],
        thickness: Option<f64>,
    ) -> Result<(), &'static str> {
        self.can_propose_shroud(thickness)?;
        let wall = thickness.unwrap_or(0.0);
        let mut next = self.clone();
        for k in 0..=SHROUD_OFFSET_STEPS {
            let t = 1.0 - k as f64 / SHROUD_OFFSET_STEPS as f64;
            let at = offset_point(points, t, wall);
            if next.extend(points, End::Finish, at).is_none() {
                return Err("el offset no cabe en la tapa");
            }
        }
        next.closed = true;
        *self = next;
        Ok(())
    }

    /// Si el tramo `piece` es de la prolongación.
    fn extension_piece(&self, piece: usize) -> bool {
        piece < self.head || piece >= self.pieces.len() - self.tail
    }

    /// El `t` del nodo `index`; los de la prolongación valen lo que su
    /// extremo de la Bézier.
    fn node_t(&self, index: usize) -> f64 {
        if index <= self.p0() {
            0.0
        } else if index < self.p4() {
            self.cuts[index - self.head - 1]
        } else {
            1.0
        }
    }

    /// Si la punta de la prolongación `end` admite una recta nueva: la de
    /// una prolongación sí; P0 o P4, solo si siguen sobre la Bézier.
    pub(super) fn can_extend(&self, end: End) -> bool {
        self.free[self.tip(end)].is_none() || self.is_extension(self.tip(end))
    }

    /// El nodo de la punta de la cadena.
    pub(super) fn tip(&self, end: End) -> usize {
        match end {
            End::Start => 0,
            End::Finish => self.node_count() - 1,
        }
    }

    /// De qué punta es el nodo `index`, si es una.
    pub(super) fn tip_end(&self, index: usize) -> Option<End> {
        if index == 0 {
            Some(End::Start)
        } else if index + 1 == self.node_count() {
            Some(End::Finish)
        } else {
            None
        }
    }

    /// Añade en la punta `end` un nodo libre en `at` (la r no baja de 0) y una
    /// recta hasta la punta de antes. Devuelve el índice del nodo nuevo, o
    /// `None` si la punta no admite recta o el tramo sería demasiado corto.
    pub(super) fn extend(
        &mut self,
        points: &[[f64; 2]; 5],
        end: End,
        at: [f64; 2],
    ) -> Option<usize> {
        if !self.can_extend(end) {
            return None;
        }
        let at = [at[0], at[1].max(0.0)];
        if distance(at, self.node_at(points, self.tip(end))) < MIN_LENGTH_MM {
            return None;
        }
        let index = match end {
            End::Start => {
                self.free.insert(0, Some(at));
                self.radius.insert(0, None);
                self.chamfer.insert(0, None);
                self.pieces.insert(0, Piece::Line);
                self.tangent.insert(0, None);
                self.head += 1;
                0
            }
            End::Finish => {
                self.free.push(Some(at));
                self.radius.push(None);
                self.chamfer.push(None);
                self.pieces.push(Piece::Line);
                self.tangent.push(None);
                self.tail += 1;
                self.node_count() - 1
            }
        };
        self.settle();
        Some(index)
    }

    /// Dónde está el nodo `index`: libre, o sobre la Bézier en su `t`. El
    /// nodo lejano de un arco tangente va a su círculo.
    pub(super) fn node_at(&self, points: &[[f64; 2]; 5], index: usize) -> [f64; 2] {
        let at = self.free[index].unwrap_or_else(|| point_at(points, self.node_t(index)));
        self.tangent_circle(points, index)
            .map_or(at, |circle| circle.project(at))
    }

    /// El nodo donde el arco `piece` es tangente a su vecino.
    fn anchor_of(piece: usize, tangency: Tangency) -> usize {
        match tangency {
            Tangency::Start => piece,
            Tangency::End => piece + 1,
        }
    }

    /// El nodo del otro extremo del arco `piece`, que queda sobre su círculo.
    pub(super) fn far_of(piece: usize, tangency: Tangency) -> usize {
        match tangency {
            Tangency::Start => piece + 1,
            Tangency::End => piece,
        }
    }

    /// El arco tangente que lleva el nodo `index` sobre su círculo.
    fn driver(&self, index: usize) -> Option<(usize, Tangency)> {
        self.tangent
            .iter()
            .enumerate()
            .find_map(|(piece, tangent)| {
                tangent
                    .filter(|t| Self::far_of(piece, *t) == index)
                    .map(|t| (piece, t))
            })
    }

    /// Los nodos de los que sale el círculo que lleva el nodo `index`; vacío
    /// si no va sobre ninguno.
    pub(super) fn circle_inputs(&self, index: usize) -> Vec<usize> {
        self.driver(index)
            .map(|(piece, tangency)| self.tangent_inputs(piece, tangency))
            .unwrap_or_default()
    }

    /// Si el nodo `index` es donde un arco sale tangente a su vecino.
    fn is_anchor(&self, index: usize) -> bool {
        self.tangent
            .iter()
            .enumerate()
            .any(|(piece, tangent)| tangent.is_some_and(|t| Self::anchor_of(piece, t) == index))
    }

    /// Los nodos de los que depende el círculo del arco tangente `piece`: el
    /// de la tangencia y el otro extremo del vecino si es recta o arco.
    fn tangent_inputs(&self, piece: usize, tangency: Tangency) -> Vec<usize> {
        match tangency {
            Tangency::Start if self.pieces[piece - 1].rigid() => vec![piece - 1, piece],
            Tangency::Start => vec![piece],
            Tangency::End if self.pieces[piece + 1].rigid() => vec![piece + 1, piece + 2],
            Tangency::End => vec![piece + 1],
        }
    }

    /// Si el nodo `target` decide, directa o indirectamente, dónde está el
    /// nodo `index`.
    fn depends_on(&self, index: usize, target: usize, depth: usize) -> bool {
        if index == target {
            return true;
        }
        if depth > self.node_count() {
            return true;
        }
        self.driver(index).is_some_and(|(piece, tangency)| {
            self.tangent_inputs(piece, tangency)
                .into_iter()
                .any(|k| self.depends_on(k, target, depth + 1))
        })
    }

    /// Si el arco `piece` puede salir tangente por ese lado: es un arco, hay
    /// vecino y su nodo lejano puede ir libre.
    fn can_tangent(&self, piece: usize, tangency: Tangency) -> bool {
        let neighbour = match tangency {
            Tangency::Start => piece > 0,
            Tangency::End => piece + 1 < self.pieces.len(),
        };
        matches!(self.pieces.get(piece), Some(Piece::Arc(_)))
            && neighbour
            && self.can_free(Self::far_of(piece, tangency))
    }

    /// Si el arco `piece` puede salir tangente por ese lado con las
    /// tangencias que ya hay: su nodo lejano no lo lleva otro arco y su
    /// círculo no depende de ese mismo nodo (se morderían la cola).
    fn fits(&self, piece: usize, tangency: Tangency) -> bool {
        let far = Self::far_of(piece, tangency);
        self.can_tangent(piece, tangency)
            && self.driver(far).is_none()
            && !self
                .tangent_inputs(piece, tangency)
                .into_iter()
                .any(|k| self.depends_on(k, far, 0))
    }

    /// La dirección de avance del tramo `piece` (del arranque al final) en
    /// su arranque o, con `at_end`, en su final.
    fn direction(&self, points: &[[f64; 2]; 5], piece: usize, at_end: bool) -> Option<[f64; 2]> {
        let unit = |d: [f64; 2]| {
            let n = d[0].hypot(d[1]);
            (n > 1.0e-12).then(|| [d[0] / n, d[1] / n])
        };
        match self.pieces[piece] {
            Piece::Curve => {
                let (start, end) = (self.node_t(piece), self.node_t(piece + 1));
                let t = if at_end { end } else { start };
                let h = ((end - start) * 1.0e-4).max(1.0e-9);
                let (low, high) = ((t - h).max(start), (t + h).min(end));
                let (a, b) = (point_at(points, low), point_at(points, high));
                unit([b[0] - a[0], b[1] - a[1]])
            }
            _ => {
                if let Some(arc) = self.arc(points, piece) {
                    return Some(arc.direction(if at_end { 1.0 } else { 0.0 }));
                }
                let (a, b) = (self.node_at(points, piece), self.node_at(points, piece + 1));
                unit([b[0] - a[0], b[1] - a[1]])
            }
        }
    }

    /// El círculo sobre el que va el nodo `index`, si es el lejano de un arco
    /// tangente.
    fn tangent_circle(&self, points: &[[f64; 2]; 5], index: usize) -> Option<Tangent> {
        let (piece, tangency) = self.driver(index)?;
        let Piece::Arc(radius) = self.pieces[piece] else {
            return None;
        };
        // Visto desde el nodo de la tangencia: del final hacia atrás, el arco
        // se recorre al revés y su R cambia de signo.
        let (anchor, dir, radius) = match tangency {
            Tangency::Start => (piece, self.direction(points, piece - 1, true)?, radius),
            Tangency::End => {
                let d = self.direction(points, piece + 1, false)?;
                (piece + 1, [-d[0], -d[1]], -radius)
            }
        };
        Tangent::new(self.node_at(points, anchor), dir, radius)
    }

    /// Hace el arco `piece` tangente a su vecino por ese lado, o le quita la
    /// tangencia con `None`. Devuelve si cambió.
    pub(super) fn set_tangent(
        &mut self,
        points: &[[f64; 2]; 5],
        piece: usize,
        tangency: Option<Tangency>,
    ) -> bool {
        if piece >= self.pieces.len() || self.tangent[piece] == tangency {
            return false;
        }
        // Primero se quita la que tuviera; la nueva tiene que caber con las
        // que quedan, que no se mueven.
        self.tangent[piece] = None;
        if let Some(t) = tangency {
            if !self.fits(piece, t) {
                return false;
            }
            self.tangent[piece] = tangency;
        }
        self.settle();
        // El nodo lejano se guarda ya sobre el círculo.
        if let Some(t) = tangency {
            let far = Self::far_of(piece, t);
            self.free[far] = Some(self.node_at(points, far));
        }
        true
    }

    /// Si el nodo puede quedar libre: todos sus tramos vecinos son rectas o
    /// arcos.
    pub(super) fn can_free(&self, index: usize) -> bool {
        if self.is_extension(index) {
            return true;
        }
        if (index == self.p0() && self.head > 0) || (index == self.p4() && self.tail > 0) {
            return false;
        }
        let left = index == 0 || self.pieces[index - 1].rigid();
        let right = index >= self.pieces.len() || self.pieces[index].rigid();
        left && right
    }

    /// Si el tramo `piece` es una recta.
    pub(super) fn is_line(&self, piece: usize) -> bool {
        self.pieces.get(piece) == Some(&Piece::Line)
    }

    /// Si el nodo es un punto libre de verdad: puede serlo y no lo lleva el
    /// círculo de una tangencia. Es lo que pide el hijo de una cota.
    pub(super) fn is_free_node(&self, index: usize) -> bool {
        index < self.node_count() && self.can_free(index) && self.driver(index).is_none()
    }

    /// Escribe la coordenada `axis` (0 z, 1 r, en mm) del nodo libre `index`,
    /// partiendo de donde está. Devuelve si cambió.
    pub(super) fn set_free_axis(
        &mut self,
        points: &[[f64; 2]; 5],
        index: usize,
        axis: usize,
        value: f64,
    ) -> bool {
        if !self.is_free_node(index) {
            return false;
        }
        let mut at = self.node_at(points, index);
        at[axis] = value;
        let before = self.free[index];
        self.free[index] = Some(at);
        before != self.free[index]
    }

    /// Si el nodo es una esquina que se puede redondear: interior y con una
    /// recta al menos a un lado. Entre dos curvas la Bézier es lisa.
    fn can_round(&self, index: usize) -> bool {
        self.interior(index)
            && (self.pieces[index - 1] == Piece::Line || self.pieces[index] == Piece::Line)
            && !self.is_anchor(index)
    }

    /// Pone el radio del nodo `index` (`None` lo quita). Devuelve si cambió.
    fn set_radius(&mut self, index: usize, radius: Option<f64>) -> bool {
        let radius = radius.filter(|r| *r > 0.0 && r.is_finite());
        if !self.can_round(index) && radius.is_some() {
            return false;
        }
        let before = self.radius[index];
        self.radius[index] = radius;
        if radius.is_some() {
            self.chamfer[index] = None;
        }
        before != radius
    }

    /// Si el nodo es una esquina entre dos rectas, que se puede achaflanar.
    fn can_chamfer(&self, index: usize) -> bool {
        self.interior(index)
            && self.pieces[index - 1] == Piece::Line
            && self.pieces[index] == Piece::Line
            && !self.is_anchor(index)
    }

    /// Pone el chaflán del nodo `index` (`None` lo quita) y le quita el
    /// radio. Devuelve si cambió.
    fn set_chamfer(&mut self, index: usize, chamfer: Option<f64>) -> bool {
        let chamfer = chamfer.filter(|d| *d > 0.0 && d.is_finite());
        if !self.can_chamfer(index) && chamfer.is_some() {
            return false;
        }
        let (before, radius) = (self.chamfer[index], self.radius[index]);
        self.chamfer[index] = chamfer;
        if chamfer.is_some() {
            self.radius[index] = None;
        }
        before != chamfer || (chamfer.is_some() && radius.is_some())
    }

    /// Si el nodo se puede arrastrar: libre o deslizándose (un corte). Los
    /// extremos pegados a una curva son los de la Bézier y no se mueven aquí.
    fn draggable(&self, index: usize) -> bool {
        self.can_free(index) || self.cut_slot(index).is_some()
    }

    /// Si el tramo puede volver a ser curva: los de la prolongación, no.
    pub(super) fn can_be_curve(&self, piece: usize) -> bool {
        !self.extension_piece(piece)
    }

    /// Si el nodo tiene un tramo a cada lado (no es una punta de la cadena).
    fn interior(&self, index: usize) -> bool {
        (1..self.node_count().saturating_sub(1)).contains(&index)
    }

    /// Devuelve a la Bézier los nodos libres que ya no pueden serlo y quita
    /// el radio a los que ya no son esquina.
    fn settle(&mut self) {
        self.closed &= self.head + self.tail > 0;
        for piece in 0..self.pieces.len() {
            if self.extension_piece(piece) && self.pieces[piece] == Piece::Curve {
                self.pieces[piece] = Piece::Line;
            }
        }
        // Las tangencias se vuelven a poner del arranque al final, y solo
        // las que caben con las anteriores.
        let tangents = std::mem::replace(&mut self.tangent, vec![None; self.pieces.len()]);
        for (piece, tangency) in tangents.into_iter().enumerate() {
            if let Some(tangency) = tangency.filter(|t| self.fits(piece, *t)) {
                self.tangent[piece] = Some(tangency);
            }
        }
        for index in 0..self.node_count() {
            if !self.can_free(index) {
                self.free[index] = None;
            }
            if !self.can_round(index) {
                self.radius[index] = None;
            }
            if !self.can_chamfer(index) {
                self.chamfer[index] = None;
            }
        }
    }

    /// Por qué no se puede cortar el tramo `piece` en `s` (el `t` de la
    /// Bézier si es curva, la fracción de su largo si es recta o arco), o
    /// `None`.
    fn cut_refusal(&self, piece: usize, s: f64) -> Option<&'static str> {
        if self.extension_piece(piece) {
            return (s < MIN_GAP || 1.0 - s < MIN_GAP)
                .then_some("demasiado cerca de otro punto: aleja el cursor");
        }
        let (start, end) = (self.node_t(piece), self.node_t(piece + 1));
        let t = if self.pieces[piece].rigid() {
            start + (end - start) * s
        } else {
            s
        };
        (t - start < MIN_GAP || end - t < MIN_GAP)
            .then_some("demasiado cerca de otro punto: aleja el cursor")
    }

    /// Corta el tramo `piece` en `s`. Una curva se parte en dos curvas que
    /// dibujan lo mismo; una recta, en dos rectas con un punto libre en medio;
    /// un arco, en dos arcos del mismo radio.
    fn cut(&mut self, points: &[[f64; 2]; 5], piece: usize, s: f64) -> bool {
        if self.cut_refusal(piece, s).is_some() {
            return false;
        }
        let kind = self.pieces[piece];
        let extension = self.extension_piece(piece);
        let (t, free) = if extension {
            (0.0, Some(self.rigid_at(points, piece, s)))
        } else if kind.rigid() {
            let (start, end) = (self.node_t(piece), self.node_t(piece + 1));
            (
                start + (end - start) * s,
                Some(self.rigid_at(points, piece, s)),
            )
        } else {
            (s, None)
        };
        if !extension {
            self.cuts.insert(piece - self.head, t);
        } else if piece < self.head {
            self.head += 1;
        } else {
            self.tail += 1;
        }
        self.pieces.insert(piece, kind);
        self.free.insert(piece + 1, free);
        self.radius.insert(piece + 1, None);
        self.chamfer.insert(piece + 1, None);
        // Cada mitad conserva la tangencia del lado que tenía.
        let tangent = self.tangent[piece];
        self.tangent[piece] = tangent.filter(|t| *t == Tangency::Start);
        self.tangent
            .insert(piece + 1, tangent.filter(|t| *t == Tangency::End));
        self.settle();
        true
    }

    /// Corta en `t` sobre la Bézier, si cae en un tramo curvo.
    pub(super) fn cut_at_t(&mut self, points: &[[f64; 2]; 5], t: f64) -> bool {
        let piece = self.head + self.cuts.iter().take_while(|cut| **cut < t).count();
        self.pieces[piece] == Piece::Curve && self.cut(points, piece, t)
    }

    /// Pasa el tramo `piece` a recta si es curva, o a curva si no. Al volver
    /// a curva, sus nodos vuelven a la Bézier.
    pub(super) fn toggle(&mut self, piece: usize) {
        let kind = if self.pieces[piece] == Piece::Curve {
            Piece::Line
        } else {
            Piece::Curve
        };
        self.set_piece(piece, kind);
    }

    /// Cambia el tramo `piece` a `kind`. Devuelve si cambió.
    pub(super) fn set_piece(&mut self, piece: usize, kind: Piece) -> bool {
        if piece >= self.pieces.len() || self.pieces[piece] == kind {
            return false;
        }
        self.pieces[piece] = kind;
        self.settle();
        true
    }

    /// El arco del tramo `piece`, si lo es y su radio cabe.
    fn arc(&self, points: &[[f64; 2]; 5], piece: usize) -> Option<Arc> {
        match self.pieces.get(piece) {
            Some(Piece::Arc(r)) => Arc::new(
                self.node_at(points, piece),
                self.node_at(points, piece + 1),
                *r,
            ),
            _ => None,
        }
    }

    /// Si el tramo `piece` es un arco cuyo radio no llega a media cuerda.
    fn arc_refused(&self, points: &[[f64; 2]; 5], piece: usize) -> bool {
        matches!(self.pieces.get(piece), Some(Piece::Arc(_))) && self.arc(points, piece).is_none()
    }

    /// El punto en la fracción `f` de un tramo recto o en arco; un arco que
    /// no cabe se toma por su cuerda.
    fn rigid_at(&self, points: &[[f64; 2]; 5], piece: usize, f: f64) -> [f64; 2] {
        if let Some(arc) = self.arc(points, piece) {
            return arc.at(f);
        }
        let (a, b) = (self.node_at(points, piece), self.node_at(points, piece + 1));
        [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
    }

    /// El radio que se propone al pasar el tramo `piece` a arco: el del
    /// círculo por sus dos nodos y el punto medio de la Bézier, redondeado.
    /// Nunca menos de media cuerda.
    fn proposed_arc(&self, points: &[[f64; 2]; 5], piece: usize) -> f64 {
        let (a, b) = (self.node_at(points, piece), self.node_at(points, piece + 1));
        let middle = point_at(points, (self.node_t(piece) + self.node_t(piece + 1)) / 2.0);
        let half = distance(a, b) / 2.0;
        let radius = meridian_arc::through(a, middle, b).unwrap_or(4.0 * half);
        let step = if radius.abs() >= 10.0 { 1.0 } else { 0.1 };
        let size = ((radius.abs() / step).round() * step).max((half / step).ceil() * step);
        size.max(step).copysign(radius)
    }

    /// Pone el radio |`value`| al arco `piece`, sin cambiar hacia dónde se
    /// curva. Devuelve si cambió.
    fn set_arc_radius(&mut self, piece: usize, value: f64) -> bool {
        match self.pieces.get(piece) {
            Some(Piece::Arc(r)) if value.is_finite() && value > 0.0 => {
                let r = *r;
                self.set_piece(piece, Piece::Arc(value.copysign(r)))
            }
            _ => false,
        }
    }

    /// Curva el arco `piece` hacia el otro lado.
    fn flip_arc(&mut self, piece: usize) -> bool {
        match self.pieces.get(piece) {
            Some(Piece::Arc(r)) => {
                let r = *r;
                self.set_piece(piece, Piece::Arc(-r))
            }
            _ => false,
        }
    }

    /// Lleva el nodo `index` hacia `to`: libre si puede, si no deslizándose
    /// sobre la Bézier sin pasar a sus vecinos, al punto más cercano.
    /// Devuelve si se movió.
    fn move_node(&mut self, points: &[[f64; 2]; 5], index: usize, to: [f64; 2]) -> bool {
        if index >= self.node_count() {
            return false;
        }
        if self.can_free(index) {
            let to = self
                .tangent_circle(points, index)
                .map_or(to, |circle| circle.project(to));
            let before = self.free[index];
            self.free[index] = Some(to);
            return before != self.free[index];
        }
        let Some((low, high)) = self.slide_range(index) else {
            return false;
        };
        let (t, _) = nearest_t(points, to, low, high, distance);
        self.set_sliding_t(index, t)
    }

    /// Entre qué `t` se puede deslizar el nodo `index`: hasta los vecinos que
    /// están sobre la curva, dejando sitio a los libres de en medio, cuyo `t`
    /// solo ordena. `None` si no se desliza.
    fn slide_range(&self, index: usize) -> Option<(f64, f64)> {
        if self.cut_slot(index).is_none() || self.can_free(index) {
            return None;
        }
        let (before, after) = self.bounding_nodes(index);
        let low = self.node_t(before) + MIN_GAP * (index - before) as f64;
        let high = self.node_t(after) - MIN_GAP * (after - index) as f64;
        (low <= high).then_some((low, high))
    }

    /// Los nodos más próximos a `index`, por cada lado, que están sobre la
    /// curva (o son los extremos).
    fn bounding_nodes(&self, index: usize) -> (usize, usize) {
        let last = self.node_count() - 1;
        let before = (0..index)
            .rev()
            .find(|k| *k == 0 || self.free[*k].is_none())
            .unwrap_or(0);
        let after = (index + 1..=last)
            .find(|k| *k == last || self.free[*k].is_none())
            .unwrap_or(last);
        (before, after)
    }

    /// Pone el nodo `index` en `t` y reparte los `t` de los nodos libres de
    /// en medio entre sus vecinos sobre la curva, para que sigan en orden. Los
    /// libres no se mueven: su `t` solo ordena. Devuelve si cambió.
    fn set_sliding_t(&mut self, index: usize, t: f64) -> bool {
        let slot = |k: usize| k - self.head - 1;
        let before = self.cuts[slot(index)];
        let (low_node, high_node) = self.bounding_nodes(index);
        let (low_t, high_t) = (self.node_t(low_node), self.node_t(high_node));
        self.cuts[slot(index)] = t;
        for k in low_node + 1..index {
            let f = (k - low_node) as f64 / (index - low_node) as f64;
            self.cuts[slot(k)] = low_t + (t - low_t) * f;
        }
        for k in index + 1..high_node {
            let f = (k - index) as f64 / (high_node - index) as f64;
            self.cuts[slot(k)] = t + (high_t - t) * f;
        }
        t != before
    }

    /// Desliza el nodo `index` sobre la Bézier hasta donde su z (`axis` 0) o
    /// su r (`axis` 1) vale `value`, o lo más cerca que deje la curva entre
    /// sus vecinos. Devuelve si se movió.
    fn slide_to(&mut self, points: &[[f64; 2]; 5], index: usize, axis: usize, value: f64) -> bool {
        let Some((low, high)) = self.slide_range(index) else {
            return false;
        };
        let mut at = [0.0; 2];
        at[axis] = value;
        let (t, _) = nearest_t(points, at, low, high, |p, q| (p[axis] - q[axis]).abs());
        self.set_sliding_t(index, t)
    }

    /// Arrastre del nodo `index` desde `from` (donde estaba al agarrarlo)
    /// hasta el cursor `to`. Libre, va al cursor. Sobre la curva, sigue la z
    /// del cursor si el arrastre es más horizontal que vertical, y su r si
    /// no: el punto queda justo debajo o al lado del ratón. Ir al punto más
    /// cercano lo dejaba atrás donde la curva se tuerce. Devuelve si se movió.
    fn drag_to(
        &mut self,
        points: &[[f64; 2]; 5],
        index: usize,
        from: [f64; 2],
        to: [f64; 2],
    ) -> bool {
        if self.can_free(index) {
            return self.move_node(points, index, to);
        }
        let axis = usize::from((to[0] - from[0]).abs() < (to[1] - from[1]).abs());
        self.slide_to(points, index, axis, to[axis])
    }

    /// Pone la cota `kind` del nodo `index` en `value` mm. Un nodo libre va a
    /// la z o al Ø escritos sin tocar la otra coordenada; uno sobre la curva
    /// se desliza hasta ellos. Devuelve si se movió.
    fn set_dimension(
        &mut self,
        points: &[[f64; 2]; 5],
        index: usize,
        kind: Dimension,
        value: f64,
    ) -> bool {
        if index >= self.node_count() || !value.is_finite() {
            return false;
        }
        let at = self.node_at(points, index);
        let free = self.can_free(index);
        // El nodo lejano de un arco tangente va al punto de su círculo con
        // esa z o ese Ø; el largo de una recta lo sacaría del círculo.
        if let Some(circle) = self.tangent_circle(points, index) {
            let to = match kind {
                Dimension::Z => circle.crossing(0, value, at),
                Dimension::Diameter if value >= 0.0 => circle.crossing(1, value / 2.0, at),
                Dimension::Arc => return self.set_arc_radius(index, value),
                _ => None,
            };
            return to.is_some_and(|to| self.move_node(points, index, to));
        }
        match kind {
            Dimension::Z if free => self.move_node(points, index, [value, at[1]]),
            Dimension::Z => self.slide_to(points, index, 0, value),
            Dimension::Diameter if value < 0.0 => false,
            Dimension::Diameter if free => self.move_node(points, index, [at[0], value / 2.0]),
            Dimension::Diameter => self.slide_to(points, index, 1, value / 2.0),
            Dimension::Arc => self.set_arc_radius(index, value),
        }
    }

    /// Devuelve el nodo `index` a la Bézier, en su `t`.
    fn unfree(&mut self, index: usize) -> bool {
        if self.is_extension(index) {
            return false;
        }
        self.free.get_mut(index).and_then(Option::take).is_some()
    }

    /// Quita el nodo interior `index` y une sus dos tramos: recta si los dos
    /// lo eran, arco (con el radio del primero que lo sea) si los dos son
    /// rectas o arcos, y curva si no.
    fn remove_node(&mut self, index: usize) -> bool {
        if !self.can_remove(index) {
            return false;
        }
        if let Some(end) = self.tip_end(index) {
            // La punta de la prolongación: se va con su recta.
            let piece = if end == End::Start { 0 } else { index - 1 };
            self.free.remove(index);
            self.radius.remove(index);
            self.chamfer.remove(index);
            self.pieces.remove(piece);
            self.tangent.remove(piece);
            if end == End::Start {
                self.head -= 1;
            } else {
                self.tail -= 1;
            }
            self.settle();
            return true;
        }
        if let Some(slot) = self.cut_slot(index) {
            self.cuts.remove(slot);
        } else if index < self.head {
            self.head -= 1;
        } else {
            self.tail -= 1;
        }
        self.free.remove(index);
        self.radius.remove(index);
        self.chamfer.remove(index);
        let merged = match (self.pieces[index - 1], self.pieces[index]) {
            (Piece::Line, Piece::Line) => Piece::Line,
            (Piece::Arc(r), other) | (other, Piece::Arc(r)) if other.rigid() => Piece::Arc(r),
            _ => Piece::Curve,
        };
        self.pieces.remove(index);
        self.pieces[index - 1] = merged;
        let start = self.tangent[index - 1].filter(|t| *t == Tangency::Start);
        let end = self.tangent[index].filter(|t| *t == Tangency::End);
        self.tangent.remove(index);
        self.tangent[index - 1] = start.or(end);
        self.settle();
        true
    }

    /// Si el nodo se puede quitar: un corte, un nodo de la prolongación o su
    /// punta; nunca P0 ni P4.
    pub(super) fn can_remove(&self, index: usize) -> bool {
        index < self.node_count() && index != self.p0() && index != self.p4()
    }

    /// La esquina de cada nodo. Un radio que se cruza con el del otro
    /// extremo de un tramo no cabe en ninguno de los dos.
    fn corners(&self, points: &[[f64; 2]; 5]) -> Vec<Corner> {
        let mut corners: Vec<Corner> = (0..self.node_count())
            .map(|index| match (self.radius[index], self.chamfer[index]) {
                (Some(radius), _) if self.can_round(index) => self
                    .corner_fillet(points, index, radius)
                    .map_or(Corner::TooBig(radius), Corner::Round),
                (None, Some(distance)) if self.can_chamfer(index) => meridian_fillet::chamfer(
                    self.node_at(points, index),
                    self.node_at(points, index - 1),
                    self.node_at(points, index + 1),
                    distance,
                )
                .map_or(Corner::TooBigChamfer(distance), Corner::Chamfer),
                _ => Corner::Sharp,
            })
            .collect();
        // Lo que recorta cada esquina del tramo: (en su arranque, en su final).
        let trims = |corner: Corner| match corner {
            Corner::Round(f) => Some((f.sa, f.sb)),
            Corner::Chamfer(c) => Some((c.sa, c.sb)),
            _ => None,
        };
        let too_big = |corner: Corner| match corner {
            Corner::Round(f) => Corner::TooBig(f.radius),
            Corner::Chamfer(c) => Corner::TooBigChamfer(c.distance),
            other => other,
        };
        for piece in 0..self.pieces.len() {
            if let (Some(start), Some(end)) = (trims(corners[piece]), trims(corners[piece + 1]))
                && start.1 + end.0 > 1.0
            {
                corners[piece] = too_big(corners[piece]);
                corners[piece + 1] = too_big(corners[piece + 1]);
            }
        }
        corners
    }

    /// El arco del nodo interior `index`, entre el tramo `index - 1` (de
    /// entrada) y el `index` (de salida).
    fn corner_fillet(&self, points: &[[f64; 2]; 5], index: usize, radius: f64) -> Option<Fillet> {
        let vertex = self.node_at(points, index);
        let t = self.node_t(index);
        let (t_in, t_out) = (self.node_t(index - 1), self.node_t(index + 1));
        let curve_in = |s: f64| point_at(points, t - (t - t_in) * s);
        let curve_out = |s: f64| point_at(points, t + (t_out - t) * s);
        // Un arco se ve como una curva más, recorrida desde el vértice.
        let (arc_in, arc_out) = (self.arc(points, index - 1), self.arc(points, index));
        let arc_in_at = |s: f64| arc_in.map_or(vertex, |arc| arc.at(1.0 - s));
        let arc_out_at = |s: f64| arc_out.map_or(vertex, |arc| arc.at(s));
        let side_in = match self.pieces[index - 1] {
            Piece::Curve => Side::Curve(&curve_in),
            _ if arc_in.is_some() => Side::Curve(&arc_in_at),
            _ => Side::Line(self.node_at(points, index - 1)),
        };
        let side_out = match self.pieces[index] {
            Piece::Curve => Side::Curve(&curve_out),
            _ if arc_out.is_some() => Side::Curve(&arc_out_at),
            _ => Side::Line(self.node_at(points, index + 1)),
        };
        meridian_fillet::fillet(vertex, &side_in, &side_out, radius)
    }

    /// Cada tramo como polilínea, recortado por los radios de sus extremos;
    /// el arco de un radio va al final del tramo que entra en la esquina.
    fn contour(&self, points: &[[f64; 2]; 5]) -> Vec<Vec<[f64; 2]>> {
        let mut polylines = self.piece_contours(points);
        if self.closed {
            polylines.push(vec![
                self.node_at(points, self.node_count() - 1),
                self.node_at(points, 0),
            ]);
        }
        polylines
    }

    /// Los tramos de `pieces`, sin la recta de cierre.
    fn piece_contours(&self, points: &[[f64; 2]; 5]) -> Vec<Vec<[f64; 2]>> {
        let corners = self.corners(points);
        let trim = |corner: Corner, out: bool| {
            let (sa, sb) = match corner {
                Corner::Round(f) => (f.sa, f.sb),
                Corner::Chamfer(c) => (c.sa, c.sb),
                _ => (0.0, 0.0),
            };
            if out { sb } else { sa }
        };
        (0..self.pieces.len())
            .map(|piece| {
                let (from, to) = (
                    trim(corners[piece], true),
                    1.0 - trim(corners[piece + 1], false),
                );
                let mut polyline: Vec<[f64; 2]> = if self.arc(points, piece).is_some() {
                    (0..=SEGMENT_POINTS)
                        .map(|k| {
                            let f = from + (to - from) * k as f64 / SEGMENT_POINTS as f64;
                            self.rigid_at(points, piece, f)
                        })
                        .collect()
                } else if self.pieces[piece].rigid() {
                    vec![
                        self.rigid_at(points, piece, from),
                        self.rigid_at(points, piece, to),
                    ]
                } else {
                    let (start, end) = (self.node_t(piece), self.node_t(piece + 1));
                    let (start, end) = (start + (end - start) * from, start + (end - start) * to);
                    (0..=SEGMENT_POINTS)
                        .map(|k| {
                            let t = start + (end - start) * k as f64 / SEGMENT_POINTS as f64;
                            point_at(points, t)
                        })
                        .collect()
                };
                match corners[piece + 1] {
                    Corner::Round(f) => {
                        polyline.extend(meridian_fillet::arc(&f, ARC_POINTS).into_iter().skip(1));
                    }
                    Corner::Chamfer(c) => polyline.push(c.b),
                    _ => {}
                }
                polyline
            })
            .collect()
    }
}

/// Un número con `decimals` decimales, sin ceros de sobra: 0.3 y no 0.300000.
pub(super) fn trimmed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_string()
}

/// Lee una longitud con unidad en mm.
fn length_mm(text: &str) -> Option<f64> {
    hm_units::parse_quantity(text.trim())
        .ok()
        .filter(|q| q.dim() == hm_units::Dimension::LENGTH && q.si.is_finite())
        .map(|q| q.si * 1000.0)
}

/// Lee «z mm, r mm» (cualquier unidad de longitud) en mm.
fn node_position(text: &str) -> Option<[f64; 2]> {
    let (z, r) = text.split_once(',')?;
    Some([length_mm(z)?, length_mm(r)?])
}

/// Los cortes de las dos tapas.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Segments {
    pub(super) walls: [WallSegments; 2],
    /// Las cotas que mandan, de las dos tapas.
    pub(super) constraints: Constraints,
}

impl Segments {
    fn wall(&self, wall: Wall) -> &WallSegments {
        &self.walls[wall_index(wall)]
    }

    fn wall_mut(&mut self, wall: Wall) -> &mut WallSegments {
        &mut self.walls[wall_index(wall)]
    }
}

/// Los cortes escritos en `[meridian]`. Una tapa que no se entiende se toma
/// entera, sin cortes.
pub(super) fn read(boxes: &Meridian) -> Segments {
    let wall = |texts| WallSegments::from_texts(texts).unwrap_or_default();
    Segments {
        walls: [wall(&boxes.segments.shroud), wall(&boxes.segments.hub)],
        constraints: Constraints::from_texts(&boxes.segments.constraints),
    }
}

/// Escribe los cortes y las cotas en `[meridian]`. Devuelve si cambió algo.
pub(super) fn write(boxes: &mut Meridian, segments: &Segments) -> bool {
    let shroud = segments.wall(Wall::Shroud).to_texts();
    let hub = segments.wall(Wall::Hub).to_texts();
    let constraints = segments.constraints.to_texts();
    let changed = boxes.segments.shroud != shroud
        || boxes.segments.hub != hub
        || boxes.segments.constraints != constraints;
    boxes.segments.shroud = shroud;
    boxes.segments.hub = hub;
    boxes.segments.constraints = constraints;
    changed
}

pub(super) fn wall_index(wall: Wall) -> usize {
    match wall {
        Wall::Shroud => 0,
        Wall::Hub => 1,
    }
}

/// Punto de la Bézier de grado 4 en `t`, por de Casteljau.
pub(super) fn point_at(points: &[[f64; 2]; 5], t: f64) -> [f64; 2] {
    let mut p = *points;
    for level in (1..5).rev() {
        for i in 0..level {
            p[i] = [
                p[i][0] + (p[i + 1][0] - p[i][0]) * t,
                p[i][1] + (p[i + 1][1] - p[i][1]) * t,
            ];
        }
    }
    p[0]
}

/// En cuántos tramos rectos se aproxima el offset de la tapa.
const SHROUD_OFFSET_STEPS: usize = 8;

/// El punto a distancia `wall` de la Bézier en `t`, por fuera: hacia +r y −z,
/// según la normal a la tangente.
fn offset_point(points: &[[f64; 2]; 5], t: f64, wall: f64) -> [f64; 2] {
    let at = point_at(points, t);
    // La derivada de la cuártica es una cúbica de las diferencias de control.
    let diff: [[f64; 2]; 4] = std::array::from_fn(|i| {
        [
            points[i + 1][0] - points[i][0],
            points[i + 1][1] - points[i][1],
        ]
    });
    let mut d = diff;
    for level in (1..4).rev() {
        for i in 0..level {
            d[i] = [
                d[i][0] + (d[i + 1][0] - d[i][0]) * t,
                d[i][1] + (d[i + 1][1] - d[i][1]) * t,
            ];
        }
    }
    let [dz, dr] = d[0];
    let norm = dz.hypot(dr);
    if norm < 1e-9 {
        return [at[0], at[1] + wall];
    }
    [at[0] - wall * dr / norm, at[1] + wall * dz / norm]
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// El `t`, entre `low` y `high`, del punto de la Bézier más cercano a `at` y
/// su distancia según `metric`: un barrido grueso y un refinado local.
fn nearest_t(
    points: &[[f64; 2]; 5],
    at: [f64; 2],
    low: f64,
    high: f64,
    metric: impl Fn([f64; 2], [f64; 2]) -> f64,
) -> (f64, f64) {
    const SWEEP: usize = 400;
    let d = |t: f64| metric(point_at(points, t), at);
    let span = high - low;
    let mut best = (0..=SWEEP)
        .map(|k| low + span * k as f64 / SWEEP as f64)
        .map(|t| (t, d(t)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((low, f64::INFINITY));
    let mut step = span / SWEEP as f64;
    for _ in 0..30 {
        step *= 0.5;
        for t in [best.0 - step, best.0 + step] {
            let t = t.clamp(low, high);
            let value = d(t);
            if value < best.1 {
                best = (t, value);
            }
        }
    }
    best
}

/// La fracción, de 0 a 1, del punto del segmento `a`–`b` más cercano a `at`.
fn nearest_on_segment(a: [f32; 2], b: [f32; 2], at: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    if length <= f32::EPSILON {
        return 0.5;
    }
    (((at[0] - a[0]) * dx + (at[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
}

/// Un sitio del contorno: la tapa, el tramo y dónde dentro de él (el `t` de
/// la Bézier si es curva, la fracción del largo si es recta).
#[derive(Clone, Copy, Debug)]
struct Hit {
    wall: Wall,
    piece: usize,
    s: f64,
    at: [f64; 2],
}

fn to_screen(plot_ui: &egui_plot::PlotUi<'_>, p: [f64; 2]) -> egui::Pos2 {
    plot_ui.screen_from_plot(PlotPoint::new(p[0], p[1]))
}

/// El sitio del contorno mecánico más cercano a `pointer`, si queda a pocos
/// píxeles.
fn touched(
    plot_ui: &egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &Segments,
    pointer: PlotPoint,
) -> Option<Hit> {
    let screen = plot_ui.screen_from_plot(pointer);
    let metric = |p: [f64; 2], _: [f64; 2]| f64::from(to_screen(plot_ui, p).distance(screen));
    let mut best: Option<(Hit, f64)> = None;
    for wall in Wall::ALL {
        let state = segments.wall(wall);
        let points = &model.params.wall(wall).points;
        for piece in 0..state.pieces.len() {
            let hit = if state.pieces[piece].rigid() {
                // Una recta es un trozo; un arco, muchos.
                let parts = if state.arc(points, piece).is_some() {
                    SEGMENT_POINTS
                } else {
                    1
                };
                (0..parts)
                    .map(|k| {
                        let (fa, fb) = (k as f64 / parts as f64, (k + 1) as f64 / parts as f64);
                        let sa = to_screen(plot_ui, state.rigid_at(points, piece, fa));
                        let sb = to_screen(plot_ui, state.rigid_at(points, piece, fb));
                        let f = f64::from(nearest_on_segment(
                            [sa.x, sa.y],
                            [sb.x, sb.y],
                            [screen.x, screen.y],
                        ));
                        let s = fa + (fb - fa) * f;
                        let at = state.rigid_at(points, piece, s);
                        (Hit { wall, piece, s, at }, metric(at, at))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .expect("al menos un trozo")
            } else {
                let (low, high) = (state.node_t(piece), state.node_t(piece + 1));
                let (t, d) = nearest_t(points, [pointer.x, pointer.y], low, high, metric);
                (
                    Hit {
                        wall,
                        piece,
                        s: t,
                        at: point_at(points, t),
                    },
                    d,
                )
            };
            if hit.1 <= f64::from(TOUCH_PIXELS) && best.is_none_or(|(_, d)| hit.1 < d) {
                best = Some(hit);
            }
        }
    }
    best.map(|(hit, _)| hit)
}

/// El nodo a pocos píxeles de `pointer` que cumple `wanted`, de cualquier tapa.
fn near_node(
    plot_ui: &egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &Segments,
    pointer: PlotPoint,
    wanted: impl Fn(&WallSegments, usize) -> bool,
) -> Option<(Wall, usize)> {
    let screen = plot_ui.screen_from_plot(pointer);
    Wall::ALL
        .into_iter()
        .flat_map(|wall| {
            let state = segments.wall(wall);
            let points = &model.params.wall(wall).points;
            (0..state.node_count())
                .filter(|index| wanted(state, *index))
                .map(|index| {
                    let at = to_screen(plot_ui, state.node_at(points, index));
                    ((wall, index), at.distance(screen))
                })
                .collect::<Vec<_>>()
        })
        .filter(|(_, d)| *d <= TOUCH_PIXELS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(node, _)| node)
}

/// El arranque del contorno mecánico de `wall`: P0, o donde se llevó.
pub(super) fn start_of(boxes: &Meridian, model: &MeridionalModel, wall: Wall) -> [f64; 2] {
    read(boxes)
        .wall(wall)
        .node_at(&model.params.wall(wall).points, 0)
}

/// La ventana de un punto: z y Ø escritos a mano.
#[derive(Clone, Debug)]
struct Editor {
    wall: Wall,
    index: usize,
    z: String,
    /// El diámetro, Ø = 2·r, como en el plano.
    diameter: String,
    /// El radio de la esquina, en mm; vacío, viva.
    radius: String,
    /// El chaflán de la esquina, en mm; vacío, sin chaflán.
    chamfer: String,
}

fn editor_id() -> egui::Id {
    egui::Id::unique("meridian_segments_editor")
}

/// Un número en mm escrito con punto o con coma.
pub(super) fn mm_value(text: &str) -> Option<f64> {
    let text = text.trim().trim_end_matches("mm").trim().replace(',', ".");
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// El punto de la ventana abierta, si la hay.
pub(super) fn open_node(ctx: &egui::Context) -> Option<(Wall, usize)> {
    ctx.data(|data| data.get_temp::<Option<Editor>>(editor_id()))
        .flatten()
        .map(|editor| (editor.wall, editor.index))
}

/// Abre la ventana del punto `index` de `wall`.
fn open_editor(ctx: &egui::Context, wall: Wall, index: usize, at: [f64; 2], radius: Option<f64>) {
    let editor = Editor {
        wall,
        index,
        z: format!("{:.2}", at[0]),
        diameter: format!("{:.2}", 2.0 * at[1]),
        radius: radius_text(radius),
        chamfer: String::new(),
    };
    ctx.data_mut(|data| data.insert_temp(editor_id(), Some(editor)));
}

/// El radio como se escribe en la ventana: vacío si no hay.
fn radius_text(radius: Option<f64>) -> String {
    radius.map_or_else(String::new, |r| trimmed(r, 3))
}

/// La ventana del punto abierto: z y Ø en mm. Un punto libre va a los dos
/// valores; uno que se desliza por la curva va a la z o la r que cambió, y
/// la curva da la otra. Devuelve si cambió algo.
fn node_editor(
    plot_ui: &egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &mut Segments,
) -> bool {
    let ctx = plot_ui.ctx().clone();
    let Some(mut editor) = ctx
        .data(|data| data.get_temp::<Option<Editor>>(editor_id()))
        .flatten()
    else {
        return false;
    };
    let points = model.params.wall(editor.wall).points;
    let commanded = segments.constraints.fixing(editor.wall, editor.index);
    let state = segments.wall_mut(editor.wall);
    if editor.index >= state.node_count() || !state.draggable(editor.index) {
        ctx.data_mut(|data| data.remove::<Option<Editor>>(editor_id()));
        return false;
    }
    let on_arc = state.driver(editor.index).is_some();
    let free = state.can_free(editor.index) && !on_arc;
    // Las coordenadas que manda una cota no se escriben.
    let fixed = if free { commanded } else { [None; 2] };
    let at = state.node_at(&points, editor.index);
    let screen = to_screen(plot_ui, at);
    let roundable = state.can_round(editor.index);
    let chamferable = state.can_chamfer(editor.index);
    let corner = if roundable || chamferable {
        state.corners(&points)[editor.index]
    } else {
        Corner::Sharp
    };
    let z_id = editor_id().with("z");
    let r_id = editor_id().with("r");
    let radius_id = editor_id().with("radius");
    let chamfer_id = editor_id().with("chamfer");
    // Sin escribir en las cajas, siguen al punto (arrastre, deshacer...).
    let typing = ctx.memory(|memory| {
        memory.has_focus(z_id)
            || memory.has_focus(r_id)
            || memory.has_focus(radius_id)
            || memory.has_focus(chamfer_id)
    });
    if !typing {
        editor.z = format!("{:.2}", at[0]);
        editor.diameter = format!("{:.2}", 2.0 * at[1]);
        editor.radius = radius_text(state.radius[editor.index]);
        editor.chamfer = radius_text(state.chamfer[editor.index]);
    }
    let mut open = true;
    let mut apply = false;
    let title = format!("Punto {} · {}", editor.index, editor.wall.name());
    egui::Window::new(title)
        .id(editor_id().with((wall_index(editor.wall), editor.index)))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .max_width(270.0)
        .default_pos(screen + egui::vec2(18.0, 18.0))
        .show(&ctx, |ui| {
            // Los avisos largos se parten en líneas en vez de ensanchar la ventana.
            ui.set_max_width(270.0);
            egui::Grid::new(editor_id().with("grid"))
                .num_columns(3)
                .show(ui, |ui| {
                    for (axis, (label, text, id)) in [
                        ("z", &mut editor.z, z_id),
                        ("Ø", &mut editor.diameter, r_id),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        ui.label(label);
                        let field = ui.add_enabled(
                            fixed[axis].is_none(),
                            egui::TextEdit::singleline(text).id(id).desired_width(80.0),
                        );
                        if field.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        {
                            apply = true;
                        }
                        ui.label("mm");
                        ui.end_row();
                        if let Some((parent, kind)) = fixed[axis] {
                            ui.label("");
                            ui.label(
                                egui::RichText::new(crate::meridian_constraints::fixed_reason(
                                    parent, kind,
                                ))
                                .small()
                                .weak(),
                            );
                            ui.end_row();
                        }
                    }
                    if roundable {
                        ui.label("R");
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut editor.radius)
                                .id(radius_id)
                                .hint_text("viva")
                                .desired_width(80.0),
                        );
                        if field.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        {
                            apply = true;
                        }
                        ui.label("mm");
                        ui.end_row();
                    }
                    if chamferable {
                        ui.label("Chaflán");
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut editor.chamfer)
                                .id(chamfer_id)
                                .hint_text("sin chaflán")
                                .desired_width(80.0),
                        );
                        if field.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        {
                            apply = true;
                        }
                        ui.label("mm");
                        ui.end_row();
                    }
                });
            if let Corner::TooBigChamfer(_) = corner {
                ui.label(
                    egui::RichText::new("El chaflán no cabe: la esquina se dibuja viva.")
                        .small()
                        .color(REFUSED),
                );
            }
            if let Corner::TooBig(_) = corner {
                ui.label(
                    egui::RichText::new("El radio no cabe: la esquina se dibuja viva.")
                        .small()
                        .color(REFUSED),
                );
            }
            ui.label(
                egui::RichText::new(if free {
                    "Punto libre: va a la z y el Ø escritos."
                } else if on_arc {
                    "Sobre el arco tangente: cambia z o Ø y el arco da el otro."
                } else {
                    "Sobre la curva: cambia z o Ø y la curva da el otro."
                })
                .small()
                .weak(),
            );
            if ui.button("Aplicar").clicked() {
                apply = true;
            }
        });
    let mut changed = false;
    if apply {
        let (z, diameter) = (mm_value(&editor.z), mm_value(&editor.diameter));
        if free {
            // Lo que manda una cota se queda donde está.
            let z = if fixed[0].is_some() { Some(at[0]) } else { z };
            let diameter = if fixed[1].is_some() {
                Some(2.0 * at[1])
            } else {
                diameter
            };
            if let (Some(z), Some(diameter)) = (z, diameter.filter(|d| *d >= 0.0)) {
                changed = state.move_node(&points, editor.index, [z, diameter / 2.0]);
            }
        } else {
            // El valor que cambió respecto del punto, con la precisión mostrada.
            let moved = |value: Option<f64>, now: f64| value.filter(|v| (v - now).abs() >= 0.005);
            if let Some(z) = moved(z, at[0]) {
                changed = state.set_dimension(&points, editor.index, Dimension::Z, z);
            } else if let Some(diameter) = moved(diameter, 2.0 * at[1]) {
                changed = state.set_dimension(&points, editor.index, Dimension::Diameter, diameter);
            }
        }
        // Solo se aplica la caja que se tocó: si no, la otra, aún con el
        // valor de antes, desharía el cambio.
        let radius_touched = editor.radius.trim() != radius_text(state.radius[editor.index]);
        let chamfer_touched = editor.chamfer.trim() != radius_text(state.chamfer[editor.index]);
        if chamferable && chamfer_touched {
            // Vacío o cero quita el chaflán; un texto que no se entiende no
            // cambia nada.
            let text = editor.chamfer.trim();
            let chamfer = if text.is_empty() {
                Some(None)
            } else {
                mm_value(text)
                    .filter(|d| *d >= 0.0)
                    .map(|d| (d > 0.0).then_some(d))
            };
            if let Some(chamfer) = chamfer {
                changed |= state.set_chamfer(editor.index, chamfer);
            }
        } else if roundable && (radius_touched || !chamferable) {
            // Vacío o cero quita el radio; un texto que no se entiende no
            // cambia nada.
            let text = editor.radius.trim();
            let radius = if text.is_empty() {
                Some(None)
            } else {
                mm_value(text)
                    .filter(|r| *r >= 0.0)
                    .map(|r| (r > 0.0).then_some(r))
            };
            if let Some(radius) = radius {
                changed |= state.set_radius(editor.index, radius);
            }
        }
        for id in [z_id, r_id, radius_id, chamfer_id] {
            ctx.memory_mut(|memory| memory.surrender_focus(id));
        }
    }
    ctx.data_mut(|data| data.insert_temp(editor_id(), open.then_some(editor)));
    changed
}

/// El punto con la ventana abierta, para acotarlo: dónde está.
pub(super) struct NodeDimensions {
    pub(super) at: [f64; 2],
}

/// El punto que se acota: el de la ventana abierta, si la hay.
pub(super) fn selected_node(
    ctx: &egui::Context,
    model: &MeridionalModel,
    boxes: &Meridian,
) -> Option<NodeDimensions> {
    let editor = ctx
        .data(|data| data.get_temp::<Option<Editor>>(editor_id()))
        .flatten()?;
    let segments = read(boxes);
    let state = segments.wall(editor.wall);
    if editor.index >= state.node_count() || !state.draggable(editor.index) {
        return None;
    }
    let points = &model.params.wall(editor.wall).points;
    Some(NodeDimensions {
        at: state.node_at(points, editor.index),
    })
}

/// Una cota que se está escribiendo sobre el dibujo.
#[derive(Clone, Debug)]
struct Typing {
    wall: Wall,
    index: usize,
    kind: Dimension,
    text: String,
    /// El centro del valor en pantalla: ahí va la caja.
    screen: egui::Pos2,
    /// Recién abierta: toma el foco y selecciona el texto.
    fresh: bool,
}

fn typing_id() -> egui::Id {
    egui::Id::unique("meridian_segments_typing")
}

/// Clic sobre el valor de una cota del punto acotado: se abre ahí mismo para
/// escribirlo. `values` lleva, por cada valor que se puede escribir, dónde
/// está en pantalla, su cota y el texto. Devuelve si el clic cayó en uno.
pub(super) fn dimension_click(
    plot_ui: &egui_plot::PlotUi<'_>,
    values: &[(egui::Rect, Dimension, String)],
) -> bool {
    if !plot_ui.response().clicked() {
        return false;
    }
    let ctx = plot_ui.ctx().clone();
    let Some(pointer) = ctx.input(|input| input.pointer.interact_pos()) else {
        return false;
    };
    let Some(editor) = ctx
        .data(|data| data.get_temp::<Option<Editor>>(editor_id()))
        .flatten()
    else {
        return false;
    };
    let Some((rect, kind, text)) = values.iter().find(|(rect, ..)| rect.contains(pointer)) else {
        return false;
    };
    let typing = Typing {
        wall: editor.wall,
        index: editor.index,
        kind: *kind,
        text: text.trim_start_matches('Ø').to_string(),
        screen: rect.center(),
        fresh: true,
    };
    ctx.data_mut(|data| data.insert_temp(typing_id(), Some(typing)));
    true
}

/// La caja de la cota que se escribe, encima de su valor. Intro la aplica;
/// Esc o un clic fuera la cierran sin cambiar nada. Devuelve si cambió algo.
fn dimension_typing(ctx: &egui::Context, model: &MeridionalModel, segments: &mut Segments) -> bool {
    let Some(mut typing) = ctx
        .data(|data| data.get_temp::<Option<Typing>>(typing_id()))
        .flatten()
    else {
        return false;
    };
    let field_id = typing_id().with("field");
    let (mut done, mut apply) = (false, false);
    egui::Area::new(typing_id().with("area"))
        .order(egui::Order::Foreground)
        .fixed_pos(typing.screen - egui::vec2(50.0, 14.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    match typing.kind {
                        Dimension::Diameter => {
                            ui.label("Ø");
                        }
                        Dimension::Arc => {
                            ui.label("R");
                        }
                        _ => {}
                    }
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut typing.text)
                            .id(field_id)
                            .desired_width(72.0),
                    );
                    ui.label("mm");
                    if typing.fresh {
                        field.request_focus();
                        let mut state =
                            egui::text_edit::TextEditState::load(ctx, field_id).unwrap_or_default();
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(typing.text.chars().count()),
                            )));
                        state.store(ctx, field_id);
                    } else if field.lost_focus() {
                        apply = ui.input(|input| input.key_pressed(egui::Key::Enter));
                        done = true;
                    }
                });
            });
        });
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        done = true;
        apply = false;
    }
    typing.fresh = false;
    let mut changed = false;
    if apply && let Some(value) = mm_value(&typing.text) {
        let points = model.params.wall(typing.wall).points;
        let state = segments.wall_mut(typing.wall);
        if typing.index < state.node_count() {
            changed = state.set_dimension(&points, typing.index, typing.kind, value);
        }
    }
    ctx.data_mut(|data| data.insert_temp(typing_id(), (!done).then_some(typing)));
    changed
}

/// Si la tapa tiene algún tramo en recta: entonces la Bézier hidráulica se
/// dibuja de referencia, en gris, y el contorno mecánico encima.
pub(super) fn reference_only(boxes: &Meridian, wall: Wall) -> bool {
    read(boxes).wall(wall).has_rigid()
}

/// Posición de todos los nodos del contorno mecánico, para encuadrar el corte
/// aunque un punto libre salga del rectángulo de las curvas.
pub(super) fn node_positions(boxes: &Meridian, model: &MeridionalModel) -> Vec<[f64; 2]> {
    let segments = read(boxes);
    let mut out = Vec::new();
    for wall in Wall::ALL {
        let state = segments.wall(wall);
        let points = &model.params.wall(wall).points;
        out.extend((0..=state.pieces.len()).map(|node| state.node_at(points, node)));
    }
    out
}

/// Dibuja el contorno mecánico de las tapas cortadas, el tramo seleccionado
/// y los puntos: círculos los que se deslizan por la curva, cuadrados los
/// libres.
pub(super) fn draw(plot_ui: &mut egui_plot::PlotUi<'_>, model: &MeridionalModel, boxes: &Meridian) {
    let segments = read(boxes);
    let open = open_node(plot_ui.ctx());
    crate::meridian_constraints::draw(plot_ui, model, &segments, open);
    let selected: Option<(Wall, usize)> = plot_ui.ctx().data(|data| data.get_temp(selected_id()));
    for wall in Wall::ALL {
        let state = segments.wall(wall);
        if state.is_plain() && !state.has_rigid() {
            continue;
        }
        let points = &model.params.wall(wall).points;
        let colour = crate::meridian::meridian_color(wall.name());
        for (piece, polyline) in state.contour(points).into_iter().enumerate() {
            let chosen = selected == Some((wall, piece));
            plot_ui.line(
                line(
                    "Contorno mecánico",
                    polyline,
                    if chosen { SELECTED } else { colour },
                )
                .width(if chosen { 4.0 } else { 2.5 }),
            );
        }
        for (index, corner) in state.corners(points).into_iter().enumerate() {
            let (text, at, colour) = match corner {
                Corner::Sharp => continue,
                Corner::Round(f) => {
                    let middle = meridian_fillet::arc(&f, 2)[1];
                    (
                        format!("R{}", trimmed(f.radius, 2)),
                        middle,
                        Color32::from_gray(20),
                    )
                }
                Corner::Chamfer(c) => (
                    format!("C{}", trimmed(c.distance, 2)),
                    [(c.a[0] + c.b[0]) / 2.0, (c.a[1] + c.b[1]) / 2.0],
                    Color32::from_gray(20),
                ),
                Corner::TooBig(r) => (
                    format!("R{} no cabe", trimmed(r, 2)),
                    state.node_at(points, index),
                    REFUSED,
                ),
                Corner::TooBigChamfer(d) => (
                    format!("C{} no cabe", trimmed(d, 2)),
                    state.node_at(points, index),
                    REFUSED,
                ),
            };
            if matches!(corner, Corner::TooBig(_) | Corner::TooBigChamfer(_)) {
                plot_ui.points(
                    Points::new("Radio que no cabe", vec![at])
                        .radius(7.0)
                        .filled(false)
                        .color(REFUSED),
                );
            }
            plot_ui.text(
                egui_plot::Text::new("Radio", PlotPoint::new(at[0], at[1]), {
                    egui::RichText::new(format!("  {text}"))
                        .small()
                        .color(colour)
                })
                .anchor(egui::Align2::LEFT_BOTTOM),
            );
        }
        for piece in 0..state.pieces.len() {
            let Piece::Arc(radius) = state.pieces[piece] else {
                continue;
            };
            let at = arc_label_at(state, points, piece);
            let refused = state.arc_refused(points, piece);
            if refused {
                plot_ui.line(
                    line(
                        "Arco que no cabe",
                        vec![
                            state.node_at(points, piece),
                            state.node_at(points, piece + 1),
                        ],
                        REFUSED,
                    )
                    .width(2.5),
                );
            }
            let (text, colour) = if refused {
                (format!("R{} no cabe", trimmed(radius.abs(), 2)), REFUSED)
            } else {
                (format!("R{}", trimmed(radius.abs(), 2)), DIMENSION)
            };
            plot_ui.text(
                egui_plot::Text::new("Arco", PlotPoint::new(at[0], at[1]), {
                    egui::RichText::new(format!("  {text}")).color(colour)
                })
                .anchor(egui::Align2::LEFT_BOTTOM),
            );
        }
    }
}

/// Los puntos del contorno (círculos que se deslizan, cuadrados libres).
/// Van después de los extremos grises P0 y P4 para que no los tapen.
pub(super) fn draw_nodes(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    boxes: &Meridian,
) {
    let segments = read(boxes);
    for wall in Wall::ALL {
        let state = segments.wall(wall);
        if state.is_plain() && !state.has_rigid() {
            continue;
        }
        let points = &model.params.wall(wall).points;
        let (mut sliding, mut free) = (Vec::new(), Vec::new());
        for index in 0..state.node_count() {
            let at = state.node_at(points, index);
            // El nodo lejano de un arco tangente se desliza por su círculo.
            if state.can_free(index) && state.driver(index).is_none() {
                free.push(at);
            } else if state.draggable(index) {
                sliding.push(at);
            }
        }
        for (shape, nodes) in [
            (egui_plot::MarkerShape::Circle, sliding),
            (egui_plot::MarkerShape::Square, free),
        ] {
            plot_ui.points(
                Points::new("Puntos del contorno", nodes.clone())
                    .shape(shape)
                    .radius(5.0)
                    .color(Color32::WHITE),
            );
            plot_ui.points(
                Points::new("Puntos del contorno", nodes)
                    .shape(shape)
                    .radius(5.0)
                    .filled(false)
                    .color(Color32::from_gray(20)),
            );
        }
    }
}

/// Dónde va la cota del arco `piece`: en su punto medio, o en el de su
/// cuerda si no cabe.
fn arc_label_at(state: &WallSegments, points: &[[f64; 2]; 5], piece: usize) -> [f64; 2] {
    state.rigid_at(points, piece, 0.5)
}

/// El rectángulo en pantalla de la cota de un arco, que se escribe a la
/// derecha de `at` con «  R…».
fn arc_label_rect(plot_ui: &egui_plot::PlotUi<'_>, at: [f64; 2]) -> egui::Rect {
    let corner = to_screen(plot_ui, at);
    egui::Rect::from_min_max(
        corner + egui::vec2(4.0, -16.0),
        corner + egui::vec2(70.0, 2.0),
    )
}

/// El arco cuya cota está bajo el ratón.
fn arc_label_under(
    plot_ui: &egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &Segments,
) -> Option<(Wall, usize)> {
    let pointer = plot_ui.ctx().input(|input| input.pointer.interact_pos())?;
    Wall::ALL.into_iter().find_map(|wall| {
        let state = segments.wall(wall);
        let points = &model.params.wall(wall).points;
        (0..state.pieces.len())
            .filter(|piece| matches!(state.pieces[*piece], Piece::Arc(_)))
            .find(|piece| {
                arc_label_rect(plot_ui, arc_label_at(state, points, *piece)).contains(pointer)
            })
            .map(|piece| (wall, piece))
    })
}

/// Abre, sobre su cota, la caja para escribir el radio del arco `piece`.
fn open_arc_typing(
    plot_ui: &egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &Segments,
    wall: Wall,
    piece: usize,
) {
    let state = segments.wall(wall);
    let Some(Piece::Arc(radius)) = state.pieces.get(piece).copied() else {
        return;
    };
    let at = arc_label_at(state, &model.params.wall(wall).points, piece);
    let typing = Typing {
        wall,
        index: piece,
        kind: Dimension::Arc,
        text: trimmed(radius.abs(), 2),
        screen: arc_label_rect(plot_ui, at).center(),
        fresh: true,
    };
    plot_ui
        .ctx()
        .data_mut(|data| data.insert_temp(typing_id(), Some(typing)));
}

fn target_id() -> egui::Id {
    egui::Id::unique("meridian_segments_target")
}

fn selected_id() -> egui::Id {
    egui::Id::unique("meridian_segments_selected")
}

fn dragged_id() -> egui::Id {
    egui::Id::unique("meridian_segments_dragged")
}

/// Lo que se agarró: el nodo y dónde estaba al empezar (para Mayús).
#[derive(Clone, Copy, Debug)]
struct Grab {
    wall: Wall,
    index: usize,
    from: [f64; 2],
}

/// Pega `to` al eje (r = 0) y a la z o la r de `others` cuando quedan a
/// menos de `tolerance`. Devuelve el punto y, por eje, el valor al que se pegó.
fn snap(to: [f64; 2], others: &[[f64; 2]], tolerance: [f64; 2]) -> ([f64; 2], [Option<f64>; 2]) {
    let mut target = to;
    let mut guides = [None, None];
    for axis in 0..2 {
        let mut best = tolerance[axis];
        let axis_value = (axis == 1).then_some(0.0);
        for value in others.iter().map(|at| at[axis]).chain(axis_value) {
            let gap = (value - to[axis]).abs();
            if gap <= best {
                best = gap;
                target[axis] = value;
                guides[axis] = Some(value);
            }
        }
    }
    (target, guides)
}

/// Los puntos a los que se pega un nodo: todos los de ambas tapas (salvo
/// `skip`) y los de control.
fn snap_targets(
    model: &MeridionalModel,
    segments: &Segments,
    skip: Option<(Wall, usize)>,
) -> Vec<[f64; 2]> {
    Wall::ALL
        .into_iter()
        .flat_map(|wall| {
            let state = segments.wall(wall);
            let points = &model.params.wall(wall).points;
            (0..state.node_count())
                .filter(move |index| Some((wall, *index)) != skip)
                .map(move |index| state.node_at(points, index))
                .chain(points.iter().copied())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Cuánto vale `SNAP_PIXELS` en mm, por eje.
fn snap_tolerance(plot_ui: &egui_plot::PlotUi<'_>) -> [f64; 2] {
    let transform = plot_ui.transform();
    [
        f64::from(SNAP_PIXELS) / transform.dpos_dvalue_x().abs().max(f64::EPSILON),
        f64::from(SNAP_PIXELS) / transform.dpos_dvalue_y().abs().max(f64::EPSILON),
    ]
}

fn drawing_id() -> egui::Id {
    egui::Id::unique("meridian_segments_drawing")
}

/// Una prolongación en curso: el nodo nuevo sigue al ratón desde la punta
/// `end` de `wall` hasta el clic.
#[derive(Clone, Copy, Debug)]
struct Drawing {
    wall: Wall,
    end: End,
}

/// Olvida la selección y las ventanas, que dependen de los índices.
fn forget_indices(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<(Wall, usize)>(selected_id());
        data.remove::<Option<Editor>>(editor_id());
        data.remove::<Option<Typing>>(typing_id());
    });
}

/// Una prolongación en curso: la recta elástica desde la punta hasta el
/// ratón, con los imanes de siempre y Mayús para horizontal o vertical. Clic:
/// fija el nodo; Esc o clic derecho: cancela. Devuelve si cambió algo.
fn draw_extension(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &mut Segments,
    drawing: Drawing,
) -> bool {
    let ctx = plot_ui.ctx().clone();
    let response = plot_ui.response().clone();
    let points = model.params.wall(drawing.wall).points;
    let state = segments.wall(drawing.wall);
    if !state.can_extend(drawing.end) {
        ctx.data_mut(|data| data.remove::<Option<Drawing>>(drawing_id()));
        return false;
    }
    let tip = state.node_at(&points, state.tip(drawing.end));
    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) || response.secondary_clicked() {
        ctx.data_mut(|data| data.remove::<Option<Drawing>>(drawing_id()));
        return false;
    }
    let Some(pointer) = plot_ui.pointer_coordinate() else {
        return false;
    };
    let mut to = [pointer.x, pointer.y];
    if ctx.input(|input| input.modifiers.shift) {
        if (to[0] - tip[0]).abs() >= (to[1] - tip[1]).abs() {
            to[1] = tip[1];
        } else {
            to[0] = tip[0];
        }
    }
    let (mut to, guides) = snap(
        to,
        &snap_targets(model, segments, None),
        snap_tolerance(plot_ui),
    );
    to[1] = to[1].max(0.0);
    if let Some(z) = guides[0] {
        plot_ui.line(line("", vec![[z, -1.0e4], [z, 1.0e4]], SELECTED).width(1.0));
    }
    if let Some(r) = guides[1] {
        plot_ui.line(line("", vec![[-1.0e4, r], [1.0e4, r]], SELECTED).width(1.0));
    }
    plot_ui.line(line("", vec![tip, to], SELECTED).width(2.5));
    plot_ui.text(
        egui_plot::Text::new(
            "Prolongación",
            PlotPoint::new(to[0], to[1]),
            egui::RichText::new(format!(
                "  z = {:.2} mm\n  Ø = {:.2} mm\n  clic fija · Esc cancela",
                to[0],
                2.0 * to[1]
            ))
            .color(Color32::from_gray(20)),
        )
        .anchor(egui::Align2::LEFT_BOTTOM),
    );
    if !response.clicked() {
        return false;
    }
    if segments
        .wall_mut(drawing.wall)
        .extend(&points, drawing.end, to)
        .is_none()
    {
        return false;
    }
    if drawing.end == End::Start {
        segments.constraints.node_inserted(drawing.wall, 0);
    }
    crate::meridian_constraints::forget_unfinished(&ctx);
    forget_indices(&ctx);
    ctx.data_mut(|data| data.remove::<Option<Drawing>>(drawing_id()));
    true
}

/// Arrastre de un nodo. Un nodo libre sigue al ratón, se pega al eje y a la
/// z o la r de los demás puntos y, con Mayús, solo se mueve en horizontal o
/// en vertical; uno que toca una curva se desliza sobre ella. Devuelve si
/// cambió algo y si hay un nodo agarrado (entonces los puntos de control no
/// se mueven).
fn drag_node(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &mut Segments,
) -> (bool, bool) {
    let ctx = plot_ui.ctx().clone();
    let response = plot_ui.response().clone();
    if response.drag_started_by(egui::PointerButton::Primary) {
        let origin = ctx
            .input(|input| input.pointer.press_origin())
            .map(|pos| plot_ui.plot_from_screen(pos));
        let hit = origin.and_then(|origin| {
            near_node(plot_ui, model, segments, origin, WallSegments::draggable)
        });
        // Un hijo con z y Ø fijadas no tiene por dónde moverse: su ventana
        // explica quién lo fija.
        let hit = hit.filter(|(wall, index)| {
            let fixed = segments.constraints.fixing(*wall, *index);
            if fixed.iter().all(Option::is_some) {
                let state = segments.wall(*wall);
                let at = state.node_at(&model.params.wall(*wall).points, *index);
                open_editor(&ctx, *wall, *index, at, state.radius[*index]);
                crate::meridian_constraints::refuse_drag(&ctx, *wall, *index);
                return false;
            }
            true
        });
        let grabbed = hit.map(|(wall, index)| Grab {
            wall,
            index,
            from: segments
                .wall(wall)
                .node_at(&model.params.wall(wall).points, index),
        });
        ctx.data_mut(|data| data.insert_temp(dragged_id(), grabbed));
    }
    let grabbed: Option<Grab> = ctx.data(|data| data.get_temp(dragged_id())).flatten();
    if response.drag_stopped() {
        ctx.data_mut(|data| data.remove::<Option<Grab>>(dragged_id()));
    }
    let Some(grab) = grabbed else {
        if let Some(pointer) = plot_ui.pointer_coordinate()
            && near_node(plot_ui, model, segments, pointer, WallSegments::draggable).is_some()
        {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
        return (false, false);
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let points = model.params.wall(grab.wall).points;
    let mut changed = false;
    if let Some(pointer) = plot_ui.pointer_coordinate() {
        let mut to = [pointer.x, pointer.y];
        let state = segments.wall(grab.wall);
        let fixed = segments.constraints.fixing(grab.wall, grab.index);
        let here = state.node_at(&points, grab.index);
        if state.can_free(grab.index) {
            if ctx.input(|input| input.modifiers.shift) {
                let (dz, dr) = (to[0] - grab.from[0], to[1] - grab.from[1]);
                if dz.abs() >= dr.abs() {
                    to[1] = grab.from[1];
                } else {
                    to[0] = grab.from[0];
                }
            }
            let others = snap_targets(model, segments, Some((grab.wall, grab.index)));
            let (snapped, mut guides) = snap(to, &others, snap_tolerance(plot_ui));
            to = crate::meridian_constraints::lock_fixed(fixed, here, snapped);
            for (guide, parent) in guides.iter_mut().zip(fixed) {
                if parent.is_some() {
                    *guide = None;
                }
            }
            if let Some(z) = guides[0] {
                plot_ui.line(line("", vec![[z, -1.0e4], [z, 1.0e4]], SELECTED).width(1.0));
            }
            if let Some(r) = guides[1] {
                plot_ui.line(line("", vec![[-1.0e4, r], [1.0e4, r]], SELECTED).width(1.0));
            }
        }
        changed = segments
            .wall_mut(grab.wall)
            .drag_to(&points, grab.index, grab.from, to);
    }
    let at = segments.wall(grab.wall).node_at(&points, grab.index);
    plot_ui.text(
        egui_plot::Text::new(
            "Punto",
            PlotPoint::new(at[0], at[1]),
            egui::RichText::new(format!(
                "  z = {:.2} mm\n  Ø = {:.2} mm",
                at[0],
                2.0 * at[1]
            ))
            .color(Color32::from_gray(20)),
        )
        .anchor(egui::Align2::LEFT_BOTTOM),
    );
    (changed, true)
}

/// Lo que se tocó con el clic derecho.
#[derive(Clone, Copy, Debug)]
struct Target {
    hit: Hit,
    /// Un nodo a pocos píxeles del clic.
    node: Option<usize>,
}

/// Clic sobre una tapa cortada: selecciona el tramo. Clic derecho: el menú
/// con «Cortar aquí», «Convertir en recta» y lo demás. `wall_thickness` es el
/// espesor de pared del diseño en mm, para «Proponer contorno». Arrastrar un punto lo
/// mueve. Devuelve si cambió algo y si hay un punto agarrado.
pub(super) fn interact(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    boxes: &mut Meridian,
    wall_thickness: Option<f64>,
) -> (bool, bool) {
    let ctx = plot_ui.ctx().clone();
    let response = plot_ui.response().clone();
    let seeded =
        seed_from_env(&ctx, model, boxes) | crate::meridian_constraints::seed_from_env(&ctx, boxes);
    let mut segments = read(boxes);
    let drawing: Option<Drawing> = ctx.data(|data| data.get_temp(drawing_id())).flatten();
    if let Some(drawing) = drawing {
        let changed = draw_extension(plot_ui, model, &mut segments, drawing);
        return ((changed && write(boxes, &segments)) || seeded, true);
    }
    let (moved, dragging) = drag_node(plot_ui, model, &mut segments);
    if dragging {
        node_editor(plot_ui, model, &mut segments);
        return ((moved && write(boxes, &segments)) || seeded, true);
    }
    if let Some(pointer) = plot_ui.pointer_coordinate() {
        if response.secondary_clicked() {
            crate::meridian_constraints::remember_menu(&ctx);
            let target = touched(plot_ui, model, &segments, pointer).map(|hit| Target {
                hit,
                node: near_node(plot_ui, model, &segments, pointer, |_, _| true)
                    .filter(|(wall, _)| *wall == hit.wall)
                    .map(|(_, index)| index),
            });
            if let Some(target) = target {
                ctx.data_mut(|data| {
                    data.insert_temp(selected_id(), (target.hit.wall, target.hit.piece));
                });
            }
            ctx.data_mut(|data| data.insert_temp(target_id(), target));
        }
        if response.clicked() && crate::meridian_constraints::click(&ctx, model, &segments) {
            // Se abrió la caja de una cota que manda.
        } else if response.clicked()
            && ctx.input(|input| input.modifiers.shift)
            && let Some(open) = open_node(&ctx)
            && let Some(parent) = near_node(plot_ui, model, &segments, pointer, |_, _| true)
            && parent != open
        {
            // Mayús+clic: el padre de una cota del punto abierto.
            crate::meridian_constraints::pick_parent(&ctx, &segments, open, parent);
        } else if response.clicked()
            && let Some((wall, piece)) = arc_label_under(plot_ui, model, &segments)
        {
            open_arc_typing(plot_ui, model, &segments, wall, piece);
        } else if response.clicked() {
            // Clic en un punto: su ventana con z y r.
            if let Some((wall, index)) =
                near_node(plot_ui, model, &segments, pointer, WallSegments::draggable)
            {
                let state = segments.wall(wall);
                let at = state.node_at(&model.params.wall(wall).points, index);
                open_editor(&ctx, wall, index, at, state.radius[index]);
            }
            match touched(plot_ui, model, &segments, pointer) {
                Some(hit) if !segments.wall(hit.wall).is_plain() => {
                    ctx.data_mut(|data| data.insert_temp(selected_id(), (hit.wall, hit.piece)));
                }
                _ => ctx.data_mut(|data| data.remove::<(Wall, usize)>(selected_id())),
            }
        }
    }
    let target: Option<Target> = ctx.data(|data| data.get_temp(target_id())).flatten();
    let mut changed = false;
    // El arco cuyo radio se va a escribir, al salir del menú.
    let mut arc_typing: Option<(Wall, usize)> = None;
    response.context_menu(|ui| {
        if let Some(index) = crate::meridian_constraints::menu_constraint(ui.ctx()) {
            if ui.button("Borrar cota").clicked() {
                segments.constraints.remove(index);
                changed = true;
                ui.close();
            }
            ui.separator();
        }
        let Some(Target { hit, node }) = target else {
            ui.label("Haz clic derecho sobre una tapa para cortarla.");
            return;
        };
        let points = model.params.wall(hit.wall).points;
        let Segments { walls, constraints } = &mut segments;
        // Para el ángulo, que mira las dos tapas mientras `state` se edita.
        let walls_now = walls.clone();
        let angle = crate::meridian_constraints::angle_pair(
            &walls_now,
            constraints,
            hit.wall,
            hit.piece,
            hit.s,
        );
        let state = &mut walls[wall_index(hit.wall)];
        ui.label(
            egui::RichText::new(format!(
                "{} · z = {:.2} mm, Ø = {:.2} mm",
                hit.wall.name(),
                hit.at[0],
                2.0 * hit.at[1]
            ))
            .small()
            .weak(),
        );
        ui.separator();
        match state.cut_refusal(hit.piece, hit.s) {
            None => {
                if ui.button("Cortar aquí").clicked() {
                    if state.cut(&points, hit.piece, hit.s) {
                        constraints.line_lost(hit.wall, hit.piece);
                        constraints.node_inserted(hit.wall, hit.piece + 1);
                        crate::meridian_constraints::forget_unfinished(ui.ctx());
                        changed = true;
                    }
                    ui.close();
                }
            }
            Some(why) => {
                ui.add_enabled(false, egui::Button::new("Cortar aquí"))
                    .on_disabled_hover_text(why);
            }
        }
        if !state.is_plain() {
            let piece = hit.piece;
            let kind = state.pieces[piece];
            if kind == Piece::Line {
                if constraints.has_angle(hit.wall, piece) {
                    ui.label(
                        egui::RichText::new("Tiene un ángulo: cortarla o cambiarla lo borra.")
                            .small()
                            .weak(),
                    );
                }
                let pair = angle;
                let button = ui.add_enabled(pair.is_ok(), egui::Button::new("Fijar ángulo…"));
                match pair {
                    Ok(pair) if button.clicked() => {
                        crate::meridian_constraints::start_angle(ui.ctx(), model, &walls_now, pair);
                        ui.close();
                    }
                    Err(reason) => {
                        button.on_disabled_hover_text(reason);
                    }
                    Ok(_) => {}
                }
            }
            if kind != Piece::Line && ui.button("Convertir en recta").clicked() {
                changed |= state.set_piece(piece, Piece::Line);
                ui.close();
            }
            if let Piece::Arc(_) = kind {
                if ui.button("Cambiar el radio…").clicked() {
                    arc_typing = Some((hit.wall, piece));
                    ui.close();
                }
                if ui.button("Invertir arco").clicked() {
                    changed |= state.flip_arc(piece);
                    ui.close();
                }
                for (tangency, label, neighbour) in [
                    (Tangency::Start, "Tangente al tramo anterior", piece > 0),
                    (
                        Tangency::End,
                        "Tangente al tramo siguiente",
                        piece + 1 < state.pieces.len(),
                    ),
                ] {
                    if !neighbour {
                        continue;
                    }
                    let mut on = state.tangent[piece] == Some(tangency);
                    let far = WallSegments::far_of(piece, tangency);
                    let led = constraints.has_child(hit.wall, far);
                    let possible =
                        on || (!led && state.clone().set_tangent(&points, piece, Some(tangency)));
                    let response = ui.add_enabled(possible, egui::Checkbox::new(&mut on, label));
                    if response.changed() {
                        changed |= state.set_tangent(&points, piece, on.then_some(tangency));
                        ui.close();
                    }
                    response.on_disabled_hover_text(if led {
                        "el punto del otro extremo tiene cotas; bórralas antes"
                    } else {
                        "el punto del otro extremo tiene una curva al lado, o ya lo lleva otro arco"
                    });
                }
            } else if ui.button("Convertir en arco…").clicked() {
                let radius = state.proposed_arc(&points, piece);
                constraints.line_lost(hit.wall, piece);
                changed |= state.set_piece(piece, Piece::Arc(radius));
                arc_typing = Some((hit.wall, piece));
                ui.close();
            }
            if kind != Piece::Curve
                && state.can_be_curve(piece)
                && ui.button("Volver a curva").clicked()
            {
                constraints.line_lost(hit.wall, piece);
                changed |= state.set_piece(piece, Piece::Curve);
                ui.close();
            }
        }
        if let Some(index) = node {
            if let Some(end) = state.tip_end(index)
                && state.can_extend(end)
                && ui.button("Prolongar con recta").clicked()
            {
                let drawing = Some(Drawing {
                    wall: hit.wall,
                    end,
                });
                ui.ctx()
                    .data_mut(|data| data.insert_temp(drawing_id(), drawing));
                ui.close();
            }
            if state.free[index].is_some() && !state.is_extension(index) {
                let led = constraints.has_child(hit.wall, index);
                let button = ui
                    .add_enabled(!led, egui::Button::new("Devolver el punto a la curva"))
                    .on_disabled_hover_text("tiene cotas; bórralas antes");
                if button.clicked() {
                    changed |= state.unfree(index);
                    ui.close();
                }
            }
            if state.can_round(index) && ui.button("Redondear con radio…").clicked() {
                let at = state.node_at(&points, index);
                open_editor(ui.ctx(), hit.wall, index, at, state.radius[index]);
                ui.ctx()
                    .memory_mut(|memory| memory.request_focus(editor_id().with("radius")));
                ui.close();
            }
            if state.can_chamfer(index) && ui.button("Achaflanar…").clicked() {
                let at = state.node_at(&points, index);
                open_editor(ui.ctx(), hit.wall, index, at, state.radius[index]);
                ui.ctx()
                    .memory_mut(|memory| memory.request_focus(editor_id().with("chamfer")));
                ui.close();
            }
            if state.chamfer[index].is_some() && ui.button("Quitar chaflán").clicked() {
                changed |= state.set_chamfer(index, None);
                ui.close();
            }
            if state.radius[index].is_some() && ui.button("Quitar radio").clicked() {
                changed |= state.set_radius(index, None);
                ui.close();
            }
            let touching = constraints.touching(hit.wall, index);
            let label = match touching {
                0 => "Quitar este punto".to_string(),
                1 => "Quitar este punto (borra su cota)".to_string(),
                n => format!("Quitar este punto (borra sus {n} cotas)"),
            };
            if state.can_remove(index) && ui.button(label).clicked() {
                if state.remove_node(index) {
                    constraints.node_removed(hit.wall, index);
                    crate::meridian_constraints::forget_unfinished(ui.ctx());
                    changed = true;
                }
                ui.close();
            }
        }
        {
            let proposal = if hit.wall == Wall::Hub {
                state.can_propose_closed(&points, wall_thickness)
            } else {
                state.can_propose_shroud(wall_thickness)
            };
            let label = match wall_thickness {
                Some(e) => format!("Proponer contorno (pared de {e:.1} mm)"),
                None => "Proponer contorno".to_string(),
            };
            let button = ui.add_enabled(proposal.is_ok(), egui::Button::new(label));
            match proposal {
                Ok(()) if button.clicked() => {
                    let done = if hit.wall == Wall::Hub {
                        state.propose_closed(&points, wall_thickness)
                    } else {
                        state.propose_shroud(&points, wall_thickness)
                    };
                    if done.is_ok() {
                        if state.head > 0 {
                            constraints.node_inserted(hit.wall, 0);
                        }
                        crate::meridian_constraints::forget_unfinished(ui.ctx());
                        changed = true;
                    }
                    ui.close();
                }
                Err(reason) => {
                    button.on_disabled_hover_text(reason);
                }
                Ok(()) => {}
            }
        }
        let label = match constraints.touching_wall(hit.wall) {
            0 => "Quitar todos los cortes de esta tapa".to_string(),
            n => format!("Quitar todos los cortes de esta tapa (borra {n} cotas)"),
        };
        if !state.is_plain() && ui.button(label).clicked() {
            *state = WallSegments::default();
            constraints.remove_wall(hit.wall);
            crate::meridian_constraints::forget_unfinished(ui.ctx());
            changed = true;
            ui.close();
        }
    });
    if changed {
        // Los índices de los tramos y de los puntos cambian: se olvidan la
        // selección y la ventana del punto.
        forget_indices(&ctx);
    }
    if let Some((wall, piece)) = arc_typing {
        open_arc_typing(plot_ui, model, &segments, wall, piece);
    }
    changed |= node_editor(plot_ui, model, &mut segments);
    changed |= dimension_typing(&ctx, model, &mut segments);
    changed |= crate::meridian_constraints::writing(&ctx, &mut segments);
    if changed {
        return (write(boxes, &segments) || seeded, false);
    }
    (seeded, false)
}

/// Solo para las capturas: `HMF_SEGMENTS="hub:0.5:1:2=60/0"` corta el cubo
/// en `t = 0,5`, pone en recta el tramo 1 y lleva el nodo 2 (el final) a
/// z = 60 mm, r = 0. Un quinto campo abre la ventana de ese nodo y un sexto
/// pone radios con la forma `i=R` en mm; un séptimo pasa tramos a arco con
/// la forma `i=R` (R con signo, en mm), y un octavo hace tangentes arcos con
/// la forma `is` (al tramo anterior) o `ie` (al siguiente). Un noveno y un
/// décimo campo prolongan la cadena antes de P0 y después de P4 con rectas
/// a los puntos `z/r` en mm, en orden; un undécimo, con el espesor de pared en
/// mm, propone el contorno cerrado del cubo. Cortes,
/// tramos y nodos se separan con `,`; las tapas (`shroud`, `hub`), con `;`.
fn seed_from_env(ctx: &egui::Context, model: &MeridionalModel, boxes: &mut Meridian) -> bool {
    let seeded = egui::Id::unique("meridian_segments_seeded");
    if ctx.data(|data| data.get_temp::<bool>(seeded)).is_some() {
        return false;
    }
    ctx.data_mut(|data| data.insert_temp(seeded, true));
    let Ok(text) = std::env::var("HMF_SEGMENTS") else {
        return false;
    };
    let mut segments = Segments::default();
    for part in text.split(';').filter(|p| !p.trim().is_empty()) {
        let mut fields = part.split(':');
        let wall = match fields.next().map(str::trim) {
            Some("shroud") => Wall::Shroud,
            Some("hub") => Wall::Hub,
            other => {
                eprintln!("HMF_SEGMENTS: no entiendo la tapa {other:?}");
                continue;
            }
        };
        let points = model.params.wall(wall).points;
        let state = segments.wall_mut(wall);
        let mut list = || {
            fields
                .next()
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        for t in list().iter().filter_map(|t| t.parse().ok()) {
            state.cut_at_t(&points, t);
        }
        for piece in list().iter().filter_map(|i| i.parse::<usize>().ok()) {
            if piece < state.pieces.len() && state.pieces[piece] == Piece::Curve {
                state.toggle(piece);
            }
        }
        for node in list() {
            let parsed = node.split_once('=').and_then(|(index, at)| {
                let (z, r) = at.split_once('/')?;
                Some((
                    index.parse::<usize>().ok()?,
                    [z.parse().ok()?, r.parse().ok()?],
                ))
            });
            if let Some((index, at)) = parsed {
                state.move_node(&points, index, at);
            }
        }
        let editor = list().first().and_then(|i| i.parse::<usize>().ok());
        for item in list() {
            if let Some((index, radius)) = item.split_once('=')
                && let (Ok(index), Ok(radius)) = (index.parse::<usize>(), radius.parse::<f64>())
                && index < state.node_count()
            {
                state.set_radius(index, Some(radius));
            }
        }
        for item in list() {
            if let Some((piece, radius)) = item.split_once('=')
                && let (Ok(piece), Ok(radius)) = (piece.parse::<usize>(), radius.parse::<f64>())
                && radius != 0.0
            {
                state.set_piece(piece, Piece::Arc(radius));
            }
        }
        for item in list() {
            let tangency = match item.chars().last() {
                Some('s') => Tangency::Start,
                Some('e') => Tangency::End,
                _ => continue,
            };
            if let Ok(piece) = item[..item.len() - 1].parse::<usize>() {
                state.set_tangent(&points, piece, Some(tangency));
            }
        }
        for (end, items) in [(End::Start, list()), (End::Finish, list())] {
            for item in items {
                if let Some((z, r)) = item.split_once('/')
                    && let (Ok(z), Ok(r)) = (z.parse::<f64>(), r.parse::<f64>())
                {
                    state.extend(&points, end, [z, r]);
                }
            }
        }
        if let Some(wall) = list().first().and_then(|value| value.parse::<f64>().ok()) {
            let _ = state.propose_closed(&points, Some(wall));
        }
        if let Some(index) = editor
            && index < state.node_count()
        {
            open_editor(
                ctx,
                wall,
                index,
                state.node_at(&points, index),
                state.radius[index],
            );
        }
    }
    write(boxes, &segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    const POINTS: [[f64; 2]; 5] = [
        [0.0, 50.0],
        [10.0, 52.0],
        [30.0, 70.0],
        [38.0, 95.0],
        [40.0, 120.0],
    ];

    fn cut_twice() -> WallSegments {
        let mut state = WallSegments::default();
        assert!(state.cut_at_t(&POINTS, 0.3));
        assert!(state.cut_at_t(&POINTS, 0.6));
        state
    }

    #[test]
    fn de_casteljau_hits_the_ends() {
        assert_eq!(point_at(&POINTS, 0.0), POINTS[0]);
        assert_eq!(point_at(&POINTS, 1.0), POINTS[4]);
    }

    #[test]
    fn nearest_t_finds_a_point_on_the_curve() {
        let at = point_at(&POINTS, 0.37);
        let (t, d) = nearest_t(&POINTS, at, 0.0, 1.0, distance);
        assert!((t - 0.37).abs() < 1e-6, "t = {t}");
        assert!(d < 1e-6);
        let (t, _) = nearest_t(&POINTS, at, 0.5, 0.9, distance);
        assert!((t - 0.5).abs() < 1e-9, "acotado: t = {t}");
    }

    #[test]
    fn cut_pieces_draw_the_same_curve() {
        let state = cut_twice();
        let pieces = state.contour(&POINTS);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].last(), pieces[1].first());
        assert_eq!(pieces[1].last(), pieces[2].first());
        assert_eq!(pieces[0][0], POINTS[0]);
        assert_eq!(*pieces[2].last().unwrap(), POINTS[4]);
    }

    #[test]
    fn straight_piece_is_the_chord() {
        let mut state = cut_twice();
        state.toggle(1);
        let pieces = state.contour(&POINTS);
        assert_eq!(
            pieces[1],
            vec![point_at(&POINTS, 0.3), point_at(&POINTS, 0.6)]
        );
    }

    #[test]
    fn refuses_cuts_too_close() {
        let mut state = WallSegments::default();
        assert!(!state.cut_at_t(&POINTS, 0.001));
        assert!(state.cut_at_t(&POINTS, 0.5));
        assert!(!state.cut_at_t(&POINTS, 0.502));
    }

    #[test]
    fn only_nodes_between_lines_go_free() {
        let mut state = cut_twice();
        state.toggle(1);
        // Nodos 1 y 2 tocan una curva: se deslizan sobre la Bézier.
        assert!(!state.can_free(1) && !state.can_free(2));
        let off = [100.0, 100.0];
        assert!(state.move_node(&POINTS, 1, off));
        assert_eq!(state.free[1], None);
        assert!(state.cuts[0] < 0.6);
        // Con el último tramo en recta también, el nodo 2 y el final quedan libres.
        state.toggle(2);
        assert!(state.can_free(2) && state.can_free(3));
        assert!(state.move_node(&POINTS, 3, [60.0, 0.0]));
        assert_eq!(state.node_at(&POINTS, 3), [60.0, 0.0]);
        // Al volver a curva, los nodos vuelven a la Bézier.
        state.toggle(2);
        assert_eq!(state.free[3], None);
        assert_eq!(state.node_at(&POINTS, 3), POINTS[4]);
    }

    #[test]
    fn cutting_a_line_makes_two_lines_with_a_free_point() {
        let mut state = cut_twice();
        state.toggle(1);
        state.toggle(2);
        state.move_node(&POINTS, 3, [60.0, 0.0]);
        let a = state.node_at(&POINTS, 2);
        assert!(state.cut(&POINTS, 2, 0.5));
        assert_eq!(
            state.pieces,
            vec![Piece::Curve, Piece::Line, Piece::Line, Piece::Line]
        );
        let middle = state.node_at(&POINTS, 3);
        assert_eq!(middle, [(a[0] + 60.0) / 2.0, a[1] / 2.0]);
        assert!(state.can_free(3));
        // Se mueve sin tocar a los vecinos.
        assert!(state.move_node(&POINTS, 3, [55.0, 20.0]));
        assert_eq!(state.node_at(&POINTS, 4), [60.0, 0.0]);
    }

    #[test]
    fn a_sliding_cut_stays_between_its_neighbours() {
        let mut state = cut_twice();
        state.move_node(&POINTS, 1, POINTS[4]);
        assert!((state.cuts[0] - (0.6 - MIN_GAP)).abs() < 1e-6);
        state.move_node(&POINTS, 1, POINTS[0]);
        assert!((state.cuts[0] - MIN_GAP).abs() < 1e-6);
        // Los extremos pegados a una curva no se mueven.
        assert!(!state.move_node(&POINTS, 0, [5.0, 5.0]));
    }

    /// El cubo por defecto: arranca en horizontal y acaba vertical en d2.
    const HUB: [[f64; 2]; 5] = [
        [0.0, 15.17],
        [4.16, 15.17],
        [41.65, 15.17],
        [41.65, 54.62],
        [41.65, 100.0],
    ];

    #[test]
    fn dragging_along_z_carries_the_point_under_the_cursor() {
        let mut state = WallSegments::default();
        assert!(state.cut_at_t(&HUB, 0.3));
        state.toggle(0);
        state.move_node(&HUB, 0, [0.0, 0.0]);
        // El ratón se mueve en horizontal, a la altura del corte: el punto
        // sigue su z por la curva, no se queda atrás.
        let from = state.node_at(&HUB, 1);
        for z in [20.0, 30.0, 38.0] {
            assert!(state.drag_to(&HUB, 1, from, [z, from[1]]));
            let at = state.node_at(&HUB, 1);
            assert!((at[0] - z).abs() < 1e-3, "pedí z = {z} y quedó en {at:?}");
        }
        // Un arrastre más vertical sigue la r del ratón, también donde la
        // curva sube recta.
        assert!(state.drag_to(&HUB, 1, from, [from[0] + 3.0, 70.0]));
        let at = state.node_at(&HUB, 1);
        assert!((at[1] - 70.0).abs() < 1e-3, "{at:?}");
        // Y vuelve hacia atrás igual de bien.
        assert!(state.drag_to(&HUB, 1, from, [5.0, from[1]]));
        assert!((state.node_at(&HUB, 1)[0] - 5.0).abs() < 1e-3);
    }

    #[test]
    fn free_points_do_not_block_sliding_along_the_curve() {
        // Como en la captura de Jorge: el cubo llevado al eje con dos rectas
        // y un punto libre entre ellas, y luego la curva.
        let mut state = WallSegments::default();
        assert!(state.cut_at_t(&HUB, 0.4));
        state.toggle(0);
        state.move_node(&HUB, 0, [0.0, 0.0]);
        assert!(state.cut(&HUB, 0, 0.5));
        state.move_node(&HUB, 1, [20.0, 0.0]);
        let from = state.node_at(&HUB, 2);
        // El punto de la curva baja hacia z = 0 más allá del `t` del punto libre.
        assert!(state.drag_to(&HUB, 2, from, [3.0, from[1]]));
        let at = state.node_at(&HUB, 2);
        assert!((at[0] - 3.0).abs() < 1e-3, "quedó en {at:?}");
        // El punto libre no se movió y el orden sigue siendo creciente.
        assert_eq!(state.node_at(&HUB, 1), [20.0, 0.0]);
        assert!(
            state.cuts.windows(2).all(|w| w[0] < w[1]),
            "{:?}",
            state.cuts
        );
        assert!(state.slide_to(&HUB, 2, 0, 1.0));
        assert!(
            state.cuts.windows(2).all(|w| w[0] < w[1]),
            "{:?}",
            state.cuts
        );
    }

    #[test]
    fn a_point_on_the_curve_slides_to_a_written_z_or_r() {
        let mut state = cut_twice();
        let z = point_at(&POINTS, 0.45)[0];
        assert!(state.slide_to(&POINTS, 1, 0, z));
        assert!((state.node_at(&POINTS, 1)[0] - z).abs() < 1e-6);
        let r = point_at(&POINTS, 0.2)[1];
        assert!(state.slide_to(&POINTS, 1, 1, r));
        assert!((state.node_at(&POINTS, 1)[1] - r).abs() < 1e-6);
        // Más allá del vecino, se queda junto a él.
        state.slide_to(&POINTS, 1, 0, 1000.0);
        assert!((state.cuts[0] - (0.6 - MIN_GAP)).abs() < 1e-6);
        // Un punto libre no se desliza.
        state.toggle(1);
        state.toggle(0);
        assert!(!state.slide_to(&POINTS, 1, 0, z));
    }

    #[test]
    fn written_millimetres_take_comma_or_point() {
        assert_eq!(mm_value("12,5"), Some(12.5));
        assert_eq!(mm_value(" 0.25 mm"), Some(0.25));
        assert_eq!(mm_value("-3"), Some(-3.0));
        assert_eq!(mm_value("abc"), None);
    }

    #[test]
    fn removing_a_cut_merges_the_pieces() {
        let mut state = cut_twice();
        state.toggle(1);
        assert!(state.remove_node(1));
        assert_eq!(state.cuts, vec![0.6]);
        assert_eq!(state.pieces, vec![Piece::Curve, Piece::Curve]);
        assert!(state.remove_node(1));
        assert_eq!(state, WallSegments::default());
        assert!(!state.remove_node(0));
    }

    #[test]
    fn texts_round_trip() {
        let mut state = cut_twice();
        state.toggle(1);
        state.toggle(2);
        state.move_node(&POINTS, 3, [60.0, 0.0]);
        let texts = state.to_texts();
        assert_eq!(texts.cuts, vec!["0.3", "0.6"]);
        assert_eq!(texts.pieces, vec!["curve", "line", "line"]);
        assert_eq!(texts.nodes, vec!["", "", "", "60 mm, 0 mm"]);
        assert_eq!(WallSegments::from_texts(&texts), Some(state));
    }

    #[test]
    fn plain_wall_writes_nothing() {
        assert!(WallSegments::default().to_texts().is_empty());
        let mut curves = WallSegments::default();
        curves.cut_at_t(&POINTS, 0.5);
        let texts = curves.to_texts();
        assert!(texts.pieces.is_empty() && texts.nodes.is_empty());
    }

    #[test]
    fn bad_texts_read_as_the_whole_wall() {
        let texts = MeridianWallSegments {
            cuts: vec!["0.6".into(), "0.3".into()],
            ..MeridianWallSegments::default()
        };
        assert_eq!(WallSegments::from_texts(&texts), None);
        // Un nodo libre junto a una curva vuelve a la Bézier al leerlo.
        let texts = MeridianWallSegments {
            cuts: vec!["0.5".into()],
            pieces: vec!["curve".into(), "line".into()],
            nodes: vec![String::new(), "5 mm, 5 mm".into(), "60 mm, 0 mm".into()],
            ..MeridianWallSegments::default()
        };
        let state = WallSegments::from_texts(&texts).unwrap();
        assert_eq!(state.free, vec![None, None, Some([60.0, 0.0])]);
    }

    /// Cortes en 0,3 y 0,6 con el tramo del medio en recta: los nodos 1 y 2
    /// son esquinas de recta contra curva.
    #[test]
    fn dimensions_move_a_point_on_the_curve() {
        let mut state = cut_twice();
        let z = point_at(&POINTS, 0.45)[0];
        assert!(state.set_dimension(&POINTS, 1, Dimension::Z, z));
        assert!((state.node_at(&POINTS, 1)[0] - z).abs() < 1e-6);
        // El Ø es el doble de la r.
        let r = point_at(&POINTS, 0.2)[1];
        assert!(state.set_dimension(&POINTS, 1, Dimension::Diameter, 2.0 * r));
        assert!((state.node_at(&POINTS, 1)[1] - r).abs() < 1e-6);
        assert!(!state.set_dimension(&POINTS, 1, Dimension::Diameter, -1.0));
    }

    #[test]
    fn dimensions_move_a_free_point() {
        let mut state = cut_twice();
        state.toggle(0);
        state.toggle(1);
        assert!(state.move_node(&POINTS, 1, [30.0, 40.0]));
        assert!(state.set_dimension(&POINTS, 1, Dimension::Z, 35.0));
        assert!(same(state.node_at(&POINTS, 1), [35.0, 40.0]));
        assert!(state.set_dimension(&POINTS, 1, Dimension::Diameter, 100.0));
        assert!(same(state.node_at(&POINTS, 1), [35.0, 50.0]));
    }

    fn middle_line() -> WallSegments {
        let mut state = cut_twice();
        state.toggle(1);
        state
    }

    fn same(a: [f64; 2], b: [f64; 2]) -> bool {
        distance(a, b) < 1e-6
    }

    #[test]
    fn only_corners_between_two_lines_take_a_chamfer() {
        let mut state = WallSegments::default();
        state.cut_at_t(&POINTS, 0.5);
        assert!(!state.set_chamfer(1, Some(1.0)), "curva a ambos lados");
        state.set_piece(0, Piece::Line);
        assert!(!state.set_chamfer(1, Some(1.0)), "recta y curva");
        state.set_piece(1, Piece::Line);
        assert!(state.set_chamfer(1, Some(1.0)));
        assert!(!state.set_chamfer(1, Some(1.0)), "sin cambio");
        assert_eq!(state.chamfer, vec![None, Some(1.0), None]);
        // Un radio y un chaflán se excluyen.
        assert!(state.set_radius(1, Some(2.0)));
        assert_eq!(state.chamfer[1], None);
        assert!(state.set_chamfer(1, Some(1.5)));
        assert_eq!(state.radius[1], None);
    }

    #[test]
    fn chamfers_survive_the_texts_and_leave_with_the_corner() {
        let mut state = WallSegments::default();
        state.cut_at_t(&POINTS, 0.5);
        state.set_piece(0, Piece::Line);
        state.set_piece(1, Piece::Line);
        state.set_chamfer(1, Some(1.5));
        let texts = state.to_texts();
        assert_eq!(texts.chamfers, vec!["", "1.5 mm", ""]);
        assert_eq!(WallSegments::from_texts(&texts), Some(state.clone()));
        // Si el tramo deja de ser recta, la esquina deja de serlo.
        state.set_piece(1, Piece::Curve);
        assert_eq!(state.chamfer, vec![None; 3]);
        assert!(state.to_texts().chamfers.is_empty());
    }

    #[test]
    fn only_corners_with_a_line_take_a_radius() {
        let mut state = cut_twice();
        assert!(!state.can_round(1), "entre dos curvas no hay esquina");
        assert!(!state.set_radius(1, Some(2.0)));
        state.toggle(1);
        assert!(state.can_round(1) && state.can_round(2));
        assert!(
            !state.can_round(0) && !state.can_round(3),
            "los extremos no"
        );
        assert!(state.set_radius(1, Some(2.0)));
        assert!(!state.set_radius(1, Some(2.0)), "sin cambio");
        // Volver la recta a curva quita el radio.
        state.toggle(1);
        assert_eq!(state.radius, vec![None; 4]);
    }

    #[test]
    fn radii_survive_the_texts_and_the_node_edits() {
        let mut state = middle_line();
        state.set_radius(2, Some(1.5));
        let texts = state.to_texts();
        assert_eq!(texts.radii, vec!["", "", "1.5 mm", ""]);
        assert_eq!(WallSegments::from_texts(&texts), Some(state.clone()));
        // Cortar o quitar un nodo mueve los radios con sus nodos.
        assert!(state.cut_at_t(&POINTS, 0.8));
        assert_eq!(state.radius, vec![None, None, Some(1.5), None, None]);
        assert!(state.remove_node(3));
        assert_eq!(state.radius, vec![None, None, Some(1.5), None]);
        // Sin radios, `radii` no se escribe.
        assert!(cut_twice().to_texts().radii.is_empty());
        // Un radio que no se entiende hace ilegible la tapa.
        let mut bad = middle_line().to_texts();
        bad.radii = vec![String::new(), "-1 mm".into(), String::new(), String::new()];
        assert_eq!(WallSegments::from_texts(&bad), None);
    }

    #[test]
    fn a_radius_trims_both_pieces_and_joins_them_with_the_arc() {
        let mut state = middle_line();
        state.set_radius(1, Some(1.0));
        let Corner::Round(f) = state.corners(&POINTS)[1] else {
            panic!("debería caber");
        };
        let pieces = state.contour(&POINTS);
        // La curva de entrada acaba en el arco, que llega a la recta.
        assert!(same(*pieces[0].last().unwrap(), f.b) || same(*pieces[0].last().unwrap(), f.a));
        assert!(same(pieces[1][0], f.b), "{:?} {:?}", pieces[1][0], f);
        // El arco es tangente: su centro queda a R de la recta.
        let (a, b) = (state.node_at(&POINTS, 1), state.node_at(&POINTS, 2));
        let d = ((b[0] - a[0]) * (f.center[1] - a[1]) - (b[1] - a[1]) * (f.center[0] - a[0])).abs()
            / distance(a, b);
        assert!((d - 1.0).abs() < 1e-6, "d = {d}");
        // Y a R de la curva: el punto de tangencia está sobre ella.
        let (t, gap) = nearest_t(&POINTS, f.a, 0.0, 0.3, distance);
        assert!(gap < 1e-6 && t < 0.3, "t = {t}, gap = {gap}");
        assert!((distance(f.center, f.a) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_radius_that_does_not_fit_draws_the_corner_sharp() {
        let mut state = middle_line();
        let sharp = state.contour(&POINTS);
        state.set_radius(1, Some(500.0));
        assert_eq!(state.corners(&POINTS)[1], Corner::TooBig(500.0));
        assert_eq!(state.contour(&POINTS), sharp);
        // Dos radios que se cruzan en la misma recta no caben ninguno. Un
        // contorno en U de rectas: (0, 0), (0, 20), (20, 20), (20, 0).
        let mut u = cut_twice();
        for piece in 0..3 {
            u.toggle(piece);
        }
        for (index, at) in [[0.0, 0.0], [0.0, 20.0], [20.0, 20.0], [20.0, 0.0]]
            .into_iter()
            .enumerate()
        {
            assert!(u.move_node(&POINTS, index, at));
        }
        u.set_radius(1, Some(8.0));
        u.set_radius(2, Some(8.0));
        assert!(matches!(u.corners(&POINTS)[1], Corner::Round(_)));
        assert!(matches!(u.corners(&POINTS)[2], Corner::Round(_)));
        u.set_radius(1, Some(15.0));
        u.set_radius(2, Some(15.0));
        assert_eq!(
            u.corners(&POINTS)[1..3],
            [Corner::TooBig(15.0), Corner::TooBig(15.0)]
        );
    }

    /// Corte en 0,3 y 0,6 con el tramo del medio en arco de radio `radius`.
    fn middle_arc(radius: f64) -> WallSegments {
        let mut state = cut_twice();
        assert!(state.set_piece(1, Piece::Arc(radius)));
        state
    }

    #[test]
    fn arcs_survive_the_texts() {
        let mut state = middle_arc(-40.0);
        state.toggle(2);
        let texts = state.to_texts();
        assert_eq!(texts.pieces, vec!["curve", "arc", "line"]);
        assert_eq!(texts.arcs, vec!["", "-40 mm", ""]);
        assert_eq!(WallSegments::from_texts(&texts), Some(state));
        let mut bad = texts.clone();
        bad.arcs = vec![String::new(), String::new(), String::new()];
        assert_eq!(WallSegments::from_texts(&bad), None);
    }

    #[test]
    fn an_arc_joins_its_nodes_and_lets_its_ends_go_free() {
        let mut state = middle_arc(40.0);
        let (a, b) = (state.node_at(&POINTS, 1), state.node_at(&POINTS, 2));
        let polyline = &state.contour(&POINTS)[1];
        assert!(same(polyline[0], a) && same(*polyline.last().unwrap(), b));
        for p in polyline {
            let arc = state.arc(&POINTS, 1).unwrap();
            assert!((distance(*p, arc.center) - 40.0).abs() < 1e-6);
        }
        // Con una recta al otro lado, el nodo 2 queda libre.
        assert!(!state.can_free(2));
        state.toggle(2);
        assert!(state.can_free(2));
        assert!(state.move_node(&POINTS, 2, [45.0, 80.0]));
        assert!(same(
            *state.contour(&POINTS)[1].last().unwrap(),
            [45.0, 80.0]
        ));
    }

    #[test]
    fn cutting_an_arc_gives_two_arcs_on_the_same_circle() {
        let mut state = middle_arc(40.0);
        let circle = state.arc(&POINTS, 1).unwrap();
        let at = circle.at(0.5);
        assert!(state.cut(&POINTS, 1, 0.5));
        assert_eq!(state.pieces[1..3], [Piece::Arc(40.0), Piece::Arc(40.0)]);
        assert!(same(state.node_at(&POINTS, 2), at));
        for piece in [1, 2] {
            assert!(same(
                state.arc(&POINTS, piece).unwrap().center,
                circle.center
            ));
        }
        // Al quitar el corte vuelve un solo arco.
        assert!(state.remove_node(2));
        assert_eq!(state.pieces[1], Piece::Arc(40.0));
    }

    #[test]
    fn the_radius_is_written_and_flipped_keeping_its_side() {
        let mut state = middle_arc(-40.0);
        assert!(state.set_dimension(&POINTS, 1, Dimension::Arc, 614.0));
        assert_eq!(state.pieces[1], Piece::Arc(-614.0));
        assert!(!state.set_dimension(&POINTS, 1, Dimension::Arc, 0.0));
        assert!(!state.set_dimension(&POINTS, 0, Dimension::Arc, 50.0));
        assert!(state.flip_arc(1));
        assert_eq!(state.pieces[1], Piece::Arc(614.0));
    }

    #[test]
    fn a_radius_shorter_than_half_the_chord_is_refused_and_drawn_straight() {
        let state = middle_arc(1.0);
        assert!(state.arc_refused(&POINTS, 1));
        let polyline = &state.contour(&POINTS)[1];
        assert_eq!(polyline.len(), 2);
    }

    #[test]
    fn the_proposed_arc_looks_like_the_curve() {
        let state = cut_twice();
        let radius = state.proposed_arc(&POINTS, 1);
        let mut arc_state = state.clone();
        arc_state.set_piece(1, Piece::Arc(radius));
        let arc = arc_state.arc(&POINTS, 1).expect("el propuesto cabe");
        let middle = point_at(&POINTS, 0.45);
        assert!(distance(arc.at(0.5), middle) < 1.0, "{radius}");
        assert_eq!(radius, radius.round());
    }

    #[test]
    fn a_corner_between_a_line_and_an_arc_takes_a_radius() {
        let mut state = middle_arc(40.0);
        state.toggle(2);
        assert!(state.can_round(2));
        assert!(state.set_radius(2, Some(2.0)));
        let Corner::Round(f) = state.corners(&POINTS)[2] else {
            panic!("debería caber");
        };
        // Tangente al arco: los centros quedan a 40 ± 2 y la tangencia está
        // sobre el arco.
        let arc = state.arc(&POINTS, 1).unwrap();
        let gap = distance(f.center, arc.center);
        assert!(
            (gap - 38.0).abs() < 1e-3 || (gap - 42.0).abs() < 1e-3,
            "{gap} {f:?}"
        );
        assert!((distance(f.a, arc.center) - 40.0).abs() < 1e-3, "{f:?}");
        // Entre dos arcos no hay redondeo.
        let mut both = middle_arc(40.0);
        both.set_piece(2, Piece::Arc(40.0));
        assert!(!both.can_round(2));
    }

    /// Recta, arco, arco y recta, como el cubo de B1 (recta, R614, R40).
    fn line_two_arcs_line(first: f64, second: f64) -> WallSegments {
        let mut state = WallSegments::default();
        for t in [0.25, 0.5, 0.75] {
            assert!(state.cut_at_t(&POINTS, t));
        }
        assert!(state.set_piece(0, Piece::Line));
        assert!(state.set_piece(1, Piece::Arc(first)));
        assert!(state.set_piece(2, Piece::Arc(second)));
        assert!(state.set_piece(3, Piece::Line));
        state
    }

    fn smooth_at(state: &WallSegments, index: usize) -> bool {
        let into = state.direction(&POINTS, index - 1, true).unwrap();
        let out = state.direction(&POINTS, index, false).unwrap();
        same(into, out)
    }

    fn radius_kept(state: &WallSegments, piece: usize, radius: f64) -> bool {
        let arc = state.arc(&POINTS, piece).unwrap();
        let (a, b) = (
            state.node_at(&POINTS, piece),
            state.node_at(&POINTS, piece + 1),
        );
        (distance(a, arc.center) - radius).abs() < 1e-6
            && (distance(b, arc.center) - radius).abs() < 1e-6
    }

    #[test]
    fn a_chain_of_tangent_arcs_joins_smoothly_with_its_radii() {
        let mut state = line_two_arcs_line(-60.0, -20.0);
        assert!(!smooth_at(&state, 1));
        assert!(state.set_tangent(&POINTS, 1, Some(Tangency::Start)));
        assert!(state.set_tangent(&POINTS, 2, Some(Tangency::Start)));
        assert!(smooth_at(&state, 1) && smooth_at(&state, 2));
        assert!(radius_kept(&state, 1, 60.0) && radius_kept(&state, 2, 20.0));
        // Sobrevive al `.hmf`.
        let texts = state.to_texts();
        assert_eq!(texts.tangents, vec!["", "start", "start", ""]);
        let back = WallSegments::from_texts(&texts).unwrap();
        assert!(smooth_at(&back, 1) && smooth_at(&back, 2));
        // Cambiar la R mueve el nodo lejano y la cadena sigue lisa.
        assert!(state.set_arc_radius(1, 70.0));
        assert!(smooth_at(&state, 1) && smooth_at(&state, 2));
        assert!(radius_kept(&state, 1, 70.0) && radius_kept(&state, 2, 20.0));
        // Invertido, también sale tangente.
        assert!(state.flip_arc(2));
        assert!(smooth_at(&state, 2));
    }

    #[test]
    fn an_arc_tangent_at_its_end_moves_its_start() {
        let mut state = line_two_arcs_line(-60.0, -20.0);
        assert!(state.set_tangent(&POINTS, 2, Some(Tangency::End)));
        assert!(smooth_at(&state, 3));
        assert!(radius_kept(&state, 2, 20.0));
    }

    #[test]
    fn an_arc_can_leave_tangent_to_the_curve() {
        let mut state = middle_arc(-40.0);
        // Su nodo lejano tiene una curva al otro lado: no puede ir libre.
        assert!(!state.set_tangent(&POINTS, 1, Some(Tangency::Start)));
        state.toggle(2);
        assert!(state.set_tangent(&POINTS, 1, Some(Tangency::Start)));
        assert!(smooth_at(&state, 1));
        assert!(radius_kept(&state, 1, 40.0));
    }

    #[test]
    fn the_far_node_moves_only_on_the_circle() {
        let mut state = line_two_arcs_line(-60.0, -20.0);
        assert!(state.set_tangent(&POINTS, 1, Some(Tangency::Start)));
        let center = state.arc(&POINTS, 1).unwrap().center;
        assert!(state.move_node(&POINTS, 2, [5.0, 200.0]));
        assert!((distance(state.node_at(&POINTS, 2), center) - 60.0).abs() < 1e-6);
        // Una z escrita lo lleva al punto del círculo con esa z.
        let z = state.node_at(&POINTS, 2)[0] + 2.0;
        assert!(state.set_dimension(&POINTS, 2, Dimension::Z, z));
        let at = state.node_at(&POINTS, 2);
        assert!((at[0] - z).abs() < 1e-6, "{at:?}");
        assert!((distance(at, center) - 60.0).abs() < 1e-6);
    }

    #[test]
    fn clashing_tangencies_and_radii_are_refused() {
        let mut state = line_two_arcs_line(-60.0, -20.0);
        // Los dos arcos tangentes entre sí por el mismo nodo se morderían
        // la cola.
        assert!(state.set_tangent(&POINTS, 1, Some(Tangency::End)));
        assert!(!state.set_tangent(&POINTS, 2, Some(Tangency::Start)));
        // Sin vecino no hay tangencia.
        assert!(!line_two_arcs_line(-60.0, -20.0).set_tangent(&POINTS, 0, Some(Tangency::Start)));
        // En el nodo de la tangencia no hay esquina que redondear.
        let mut round = line_two_arcs_line(-60.0, -20.0);
        assert!(round.set_tangent(&POINTS, 1, Some(Tangency::Start)));
        assert!(!round.set_radius(1, Some(5.0)));
        // Al dejar de ser arco pierde la tangencia.
        assert!(round.set_piece(1, Piece::Line));
        assert_eq!(round.tangent[1], None);
    }

    #[test]
    fn cutting_or_joining_a_tangent_arc_keeps_its_side() {
        let mut state = line_two_arcs_line(-60.0, -20.0);
        assert!(state.set_tangent(&POINTS, 1, Some(Tangency::Start)));
        assert!(state.cut(&POINTS, 1, 0.5));
        assert_eq!(state.tangent[1..3], [Some(Tangency::Start), None]);
        assert!(state.remove_node(2));
        assert_eq!(state.tangent[1], Some(Tangency::Start));
        assert!(smooth_at(&state, 1));
    }

    /// Una prolongación de `head` y `tail` nodos, de rectas, para las pruebas.
    fn extended(head: usize, tail: usize) -> WallSegments {
        let mut state = WallSegments::default();
        for k in 0..head {
            let at = [POINTS[0][0] - 5.0 * (k + 1) as f64, POINTS[0][1]];
            assert!(state.extend(&POINTS, End::Start, at).is_some());
        }
        for k in 0..tail {
            let at = [POINTS[4][0] + 5.0 * (k + 1) as f64, POINTS[4][1]];
            assert!(state.extend(&POINTS, End::Finish, at).is_some());
        }
        state
    }

    #[test]
    fn extending_keeps_the_curve_whole() {
        let mut s = WallSegments::default();
        assert_eq!(s.extend(&POINTS, End::Start, [POINTS[0][0], 0.0]), Some(0));
        let at = [POINTS[4][0] + 10.0, POINTS[4][1]];
        assert_eq!(s.extend(&POINTS, End::Finish, at), Some(3));
        assert_eq!((s.head, s.tail, s.node_count()), (1, 1, 4));
        assert_eq!(s.node_at(&POINTS, s.p0()), POINTS[0]);
        assert_eq!(s.node_at(&POINTS, s.p4()), POINTS[4]);
        let c = s.contour(&POINTS);
        assert_eq!(c.len(), 3);
        assert_eq!(c[1].first(), Some(&POINTS[0]));
        assert_eq!(c[1].last(), Some(&POINTS[4]));
    }

    #[test]
    fn p0_stays_on_the_curve_with_an_extension() {
        let mut s = extended(1, 0);
        s.pieces[1] = Piece::Line;
        s.settle();
        assert!(!s.can_free(s.p0()));
        assert_eq!(s.free[s.p0()], None);
    }

    #[test]
    fn freed_p0_cannot_be_extended() {
        let mut s = WallSegments::default();
        s.toggle(0);
        assert!(s.move_node(&POINTS, 0, [0.0, 0.0]));
        assert!(!s.can_extend(End::Start));
        assert_eq!(s.extend(&POINTS, End::Start, [-5.0, 0.0]), None);
        assert!(s.can_extend(End::Finish));
    }

    #[test]
    fn too_short_extension_is_refused() {
        let mut s = WallSegments::default();
        assert_eq!(s.extend(&POINTS, End::Start, POINTS[0]), None);
        let near = [POINTS[0][0] - 0.005, POINTS[0][1]];
        assert_eq!(s.extend(&POINTS, End::Start, near), None);
        assert_eq!((s.head, s.node_count()), (0, 2));
    }

    #[test]
    fn extension_never_goes_below_the_axis() {
        let mut s = WallSegments::default();
        assert_eq!(s.extend(&POINTS, End::Start, [-5.0, -5.0]), Some(0));
        assert_eq!(s.free[0], Some([-5.0, 0.0]));
    }

    #[test]
    fn cutting_an_extension_line_grows_head() {
        let mut s = extended(1, 0);
        let cuts = s.cuts.clone();
        assert!(s.cut(&POINTS, 0, 0.5));
        assert_eq!((s.head, s.cuts.clone()), (2, cuts));
        assert!(s.free[1].is_some());
        assert_eq!(s.pieces.len(), s.node_count() - 1);
    }

    #[test]
    fn removing_the_tip_drops_its_line() {
        let mut s = extended(2, 0);
        assert!(s.remove_node(0));
        assert_eq!(s.head, 1);
        assert_eq!(s.pieces.len(), s.node_count() - 1);
        assert!(!s.remove_node(s.p0()));
        assert!(!s.remove_node(s.p4()));
    }

    #[test]
    fn p0_rounds_between_extension_and_curve() {
        let s = extended(1, 0);
        assert!(s.can_round(s.p0()));
        assert!(!s.can_chamfer(s.p0()));
        let two = extended(2, 0);
        assert!(two.can_chamfer(1));
    }

    #[test]
    fn extension_round_trips_through_texts() {
        let s = extended(2, 1);
        let back = WallSegments::from_texts(&s.to_texts()).unwrap();
        assert_eq!((back.head, back.tail), (2, 1));
        assert_eq!(back.pieces, s.pieces);
        assert_eq!(back.free, s.free);
    }

    #[test]
    fn curve_in_an_extension_becomes_a_line() {
        let texts = MeridianWallSegments {
            head: "1".into(),
            pieces: vec!["curve".into(), "curve".into()],
            nodes: vec!["-5 mm, 50 mm".into(), String::new(), String::new()],
            ..MeridianWallSegments::default()
        };
        let s = WallSegments::from_texts(&texts).unwrap();
        assert_eq!(s.pieces, vec![Piece::Line, Piece::Curve]);
    }

    #[test]
    fn proposing_closes_the_hub_to_the_axis_with_a_wall() {
        let mut s = WallSegments::default();
        assert_eq!(s.propose_closed(&POINTS, Some(5.0)), Ok(()));
        assert!(s.closed);
        let last = s.node_count() - 1;
        assert_eq!((s.head, s.tail), (1, 2));
        // La cara trasera queda a z4 + e, no encima de la curva.
        assert_eq!(s.free[0], Some([POINTS[0][0], 0.0]));
        assert_eq!(s.free[last - 1], Some([POINTS[4][0] + 5.0, POINTS[4][1]]));
        assert_eq!(s.free[last], Some([POINTS[4][0] + 5.0, 0.0]));
        let c = s.contour(&POINTS);
        assert_eq!(c.len(), s.pieces.len() + 1);
        assert_eq!(c.last().unwrap().first(), Some(&s.node_at(&POINTS, last)));
        assert_eq!(c.last().unwrap().last(), Some(&s.node_at(&POINTS, 0)));
    }

    #[test]
    fn a_zero_wall_drops_straight_to_the_axis() {
        let mut s = WallSegments::default();
        assert_eq!(s.propose_closed(&POINTS, Some(0.0)), Ok(()));
        assert_eq!((s.head, s.tail), (1, 1));
        assert_eq!(s.free[s.node_count() - 1], Some([POINTS[4][0], 0.0]));
    }

    #[test]
    fn proposing_with_p0_on_the_axis_has_no_head() {
        let mut points = POINTS;
        points[0][1] = 0.0;
        let mut s = WallSegments::default();
        assert_eq!(s.propose_closed(&points, Some(5.0)), Ok(()));
        assert_eq!((s.head, s.tail, s.closed), (0, 2, true));
    }

    #[test]
    fn proposing_is_refused_with_a_reason() {
        // P0 y P4 liberados: el cierre al eje hecho a mano.
        let mut freed = WallSegments::default();
        freed.toggle(0);
        assert!(freed.move_node(&POINTS, 0, [0.0, 0.0]));
        assert!(freed.move_node(&POINTS, 1, [40.0, 10.0]));
        let before = freed.clone();
        let why = "P0 y P4 deben seguir sobre la curva";
        assert_eq!(freed.can_propose_closed(&POINTS, Some(5.0)), Err(why));
        assert_eq!(freed.propose_closed(&POINTS, Some(5.0)), Err(why));
        assert_eq!(freed, before);
        // Con prolongación previa.
        let mut extended = extended(1, 0);
        let before = extended.clone();
        let why = "ya tiene prolongación";
        assert_eq!(extended.propose_closed(&POINTS, Some(5.0)), Err(why));
        assert_eq!(extended, before);
        // Con P0 y P4 en el eje no hay nada que cerrar.
        let mut flat = POINTS;
        flat[0][1] = 0.0;
        flat[4][1] = 0.0;
        let mut s = WallSegments::default();
        assert_eq!(
            s.propose_closed(&flat, Some(5.0)),
            Err("no hay nada que cerrar")
        );
        assert_eq!(s, WallSegments::default());
        // Sin espesor del diseño, o con uno que no vale.
        let why = "falta el espesor de pared del diseño";
        for bad in [None, Some(-1.0), Some(f64::NAN)] {
            assert_eq!(s.propose_closed(&POINTS, bad), Err(why));
        }
        assert_eq!(s, WallSegments::default());
    }

    #[test]
    fn proposing_closes_the_shroud_with_an_offset_curve() {
        let mut s = WallSegments::default();
        assert_eq!(s.propose_shroud(&POINTS, Some(5.0)), Ok(()));
        assert!(s.closed);
        assert_eq!((s.head, s.tail), (0, SHROUD_OFFSET_STEPS + 1));
        let first = s.p4() + 1;
        let last = s.node_count() - 1;
        // Cada punto está a la distancia del espesor de la curva, por fuera.
        for index in first..=last {
            let at = s.node_at(&POINTS, index);
            let (_, gap) = nearest_t(&POINTS, at, 0.0, 1.0, distance);
            assert!((gap - 5.0).abs() < 0.05, "nodo {index} a {gap}");
        }
        // Sale de P4 hacia −z y acaba encima de P0, hacia +r.
        let start = s.node_at(&POINTS, first);
        assert!(start[0] < POINTS[4][0] && (start[1] - POINTS[4][1]).abs() < 0.5);
        let end = s.node_at(&POINTS, last);
        assert!((end[0] - POINTS[0][0]).abs() < 1.5 && end[1] > POINTS[0][1]);
        let c = s.contour(&POINTS);
        assert_eq!(c.len(), s.pieces.len() + 1);
        assert_eq!(c.last().unwrap().first(), Some(&end));
        assert_eq!(c.last().unwrap().last(), Some(&POINTS[0]));
    }

    #[test]
    fn the_shroud_proposal_is_refused_with_a_reason() {
        let mut s = WallSegments::default();
        for (bad, why) in [
            (None, "falta el espesor de pared del diseño"),
            (Some(-1.0), "falta el espesor de pared del diseño"),
            (Some(f64::NAN), "falta el espesor de pared del diseño"),
            (Some(0.0), "el espesor de pared es demasiado fino"),
        ] {
            assert_eq!(s.propose_shroud(&POINTS, bad), Err(why));
        }
        assert_eq!(s, WallSegments::default());
        let mut extended = extended(1, 0);
        let before = extended.clone();
        assert_eq!(
            extended.propose_shroud(&POINTS, Some(5.0)),
            Err("ya tiene prolongación")
        );
        assert_eq!(extended, before);
        let mut freed = WallSegments::default();
        freed.toggle(0);
        assert!(freed.move_node(&POINTS, 0, [0.0, 40.0]));
        assert_eq!(
            freed.can_propose_shroud(Some(5.0)),
            Err("P0 y P4 deben seguir sobre la curva")
        );
    }

    #[test]
    fn removing_all_the_extension_opens_the_loop() {
        let mut s = WallSegments::default();
        s.propose_closed(&POINTS, Some(5.0)).unwrap();
        assert!(s.remove_node(0));
        assert!(s.closed);
        while s.tail > 0 {
            assert!(s.closed);
            assert!(s.remove_node(s.node_count() - 1));
        }
        assert!(!s.closed);
        assert_eq!(s.contour(&POINTS).len(), s.pieces.len());
    }

    #[test]
    fn closed_round_trips_through_texts() {
        let mut s = WallSegments::default();
        s.propose_closed(&POINTS, Some(5.0)).unwrap();
        let back = WallSegments::from_texts(&s.to_texts()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn closed_without_extension_is_dropped_on_read() {
        let texts = MeridianWallSegments {
            closed: "true".into(),
            ..MeridianWallSegments::default()
        };
        let s = WallSegments::from_texts(&texts).unwrap();
        assert!(!s.closed);
    }

    #[test]
    fn p0_and_p4_are_not_draggable_with_an_extension() {
        let s = extended(1, 1);
        assert!(!s.draggable(s.p0()));
        assert!(!s.draggable(s.p4()));
        assert!(s.draggable(0) && s.draggable(s.node_count() - 1));
        // Sin prolongación, como antes: los extremos pegados a la curva no se mueven.
        let plain = cut_twice();
        assert!(!plain.draggable(0) && !plain.draggable(plain.node_count() - 1));
        assert!(plain.draggable(1) && plain.draggable(2));
    }

    #[test]
    fn an_extension_piece_cannot_go_back_to_a_curve() {
        let s = extended(1, 0);
        assert!(!s.can_be_curve(0));
        assert!(s.can_be_curve(1));
    }
}
