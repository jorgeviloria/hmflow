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
//! Se guarda en `[meridian.segments]` del `.hmf`, así que entra en deshacer
//! como lo demás de la pestaña.

use eframe::egui;
use egui::Color32;
use egui_plot::{PlotPoint, Points};
use hm_design::meridian::{MeridionalModel, Wall};
use hm_project::{Meridian, MeridianWallSegments};

use crate::curves::line;

/// Distancia en píxeles para tocar una tapa o un punto con el ratón.
const TOUCH_PIXELS: f32 = 12.0;

/// Distancia en píxeles a la que un punto libre se pega al eje o a la z o la
/// r de otro punto.
const SNAP_PIXELS: f32 = 6.0;

/// Hueco mínimo, en `t`, entre dos nodos.
const MIN_GAP: f64 = 0.005;

/// Puntos con que se dibuja cada tramo curvo.
const SEGMENT_POINTS: usize = 80;

/// Color del tramo seleccionado y de las guías de ajuste.
const SELECTED: Color32 = Color32::from_rgb(230, 120, 0);

/// Los nodos y los tramos de una tapa. Hay `cuts.len() + 2` nodos (el 0 es el
/// arranque y el último el final) y `cuts.len() + 1` tramos; el tramo `i` va
/// del nodo `i` al `i + 1`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct WallSegments {
    /// El `t` de cada corte sobre la Bézier, creciente.
    cuts: Vec<f64>,
    /// Si cada tramo es recta.
    straight: Vec<bool>,
    /// La posición (z, r) en mm de cada nodo libre; `None`, sobre la Bézier.
    free: Vec<Option<[f64; 2]>>,
}

impl Default for WallSegments {
    fn default() -> Self {
        Self {
            cuts: Vec::new(),
            straight: vec![false],
            free: vec![None, None],
        }
    }
}

impl WallSegments {
    /// Lee los textos del `.hmf`; `None` si no se entienden (la validación
    /// del proyecto ya lo avisa con HM-PROJ-031).
    fn from_texts(texts: &MeridianWallSegments) -> Option<Self> {
        let mut cuts = Vec::with_capacity(texts.cuts.len());
        for text in &texts.cuts {
            let t: f64 = text.trim().parse().ok()?;
            if !(t > cuts.last().copied().unwrap_or(0.0) && t < 1.0) {
                return None;
            }
            cuts.push(t);
        }
        let straight = if texts.pieces.is_empty() {
            vec![false; cuts.len() + 1]
        } else if texts.pieces.len() == cuts.len() + 1 {
            texts
                .pieces
                .iter()
                .map(|piece| match piece.trim() {
                    "curve" => Some(false),
                    "line" => Some(true),
                    _ => None,
                })
                .collect::<Option<Vec<bool>>>()?
        } else {
            return None;
        };
        let free = if texts.nodes.is_empty() {
            vec![None; cuts.len() + 2]
        } else if texts.nodes.len() == cuts.len() + 2 {
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
        let mut state = Self {
            cuts,
            straight,
            free,
        };
        state.settle();
        Some(state)
    }

    /// Los textos para el `.hmf`: sin rectas, `pieces` va vacío; sin nodos
    /// libres, `nodes` también.
    fn to_texts(&self) -> MeridianWallSegments {
        let cuts = self.cuts.iter().map(|t| trimmed(*t, 6)).collect();
        let pieces = if self.has_straight() {
            self.straight
                .iter()
                .map(|straight| if *straight { "line" } else { "curve" }.to_string())
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
        MeridianWallSegments {
            cuts,
            pieces,
            nodes,
        }
    }

    fn is_plain(&self) -> bool {
        self.cuts.is_empty()
    }

    fn has_straight(&self) -> bool {
        self.straight.iter().any(|s| *s)
    }

    fn node_count(&self) -> usize {
        self.cuts.len() + 2
    }

    /// El `t` del nodo `index`.
    fn node_t(&self, index: usize) -> f64 {
        match index {
            0 => 0.0,
            i if i <= self.cuts.len() => self.cuts[i - 1],
            _ => 1.0,
        }
    }

    /// Dónde está el nodo `index`: libre, o sobre la Bézier en su `t`.
    fn node_at(&self, points: &[[f64; 2]; 5], index: usize) -> [f64; 2] {
        self.free[index].unwrap_or_else(|| point_at(points, self.node_t(index)))
    }

    /// Si el nodo puede quedar libre: todos sus tramos vecinos son rectas.
    fn can_free(&self, index: usize) -> bool {
        let left = index == 0 || self.straight[index - 1];
        let right = index >= self.straight.len() || self.straight[index];
        left && right
    }

    /// Si el nodo se puede arrastrar: libre o deslizándose (un corte). Los
    /// extremos pegados a una curva son los de la Bézier y no se mueven aquí.
    fn draggable(&self, index: usize) -> bool {
        self.can_free(index) || (1..=self.cuts.len()).contains(&index)
    }

    /// Devuelve a la Bézier los nodos libres que ya no pueden serlo.
    fn settle(&mut self) {
        for index in 0..self.node_count() {
            if !self.can_free(index) {
                self.free[index] = None;
            }
        }
    }

    /// Por qué no se puede cortar el tramo `piece` en `s` (el `t` de la
    /// Bézier si es curva, la fracción de su largo si es recta), o `None`.
    fn cut_refusal(&self, piece: usize, s: f64) -> Option<&'static str> {
        let (start, end) = (self.node_t(piece), self.node_t(piece + 1));
        let t = if self.straight[piece] {
            start + (end - start) * s
        } else {
            s
        };
        (t - start < MIN_GAP || end - t < MIN_GAP)
            .then_some("demasiado cerca de otro punto: aleja el cursor")
    }

    /// Corta el tramo `piece` en `s`. Una curva se parte en dos curvas que
    /// dibujan lo mismo; una recta, en dos rectas con un punto libre en medio.
    fn cut(&mut self, points: &[[f64; 2]; 5], piece: usize, s: f64) -> bool {
        if self.cut_refusal(piece, s).is_some() {
            return false;
        }
        let straight = self.straight[piece];
        let (t, free) = if straight {
            let (start, end) = (self.node_t(piece), self.node_t(piece + 1));
            let (a, b) = (self.node_at(points, piece), self.node_at(points, piece + 1));
            let at = [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s];
            (start + (end - start) * s, Some(at))
        } else {
            (s, None)
        };
        self.cuts.insert(piece, t);
        self.straight.insert(piece, straight);
        self.free.insert(piece + 1, free);
        true
    }

    /// Corta en `t` sobre la Bézier, si cae en un tramo curvo.
    fn cut_at_t(&mut self, points: &[[f64; 2]; 5], t: f64) -> bool {
        let piece = self.cuts.iter().take_while(|cut| **cut < t).count();
        !self.straight[piece] && self.cut(points, piece, t)
    }

    /// Pasa el tramo `piece` a recta, o a curva si ya lo era. Al volver a
    /// curva, sus nodos vuelven a la Bézier.
    fn toggle(&mut self, piece: usize) {
        self.straight[piece] = !self.straight[piece];
        self.settle();
    }

    /// Lleva el nodo `index` hacia `to`: libre si puede, si no deslizándose
    /// sobre la Bézier sin pasar a sus vecinos. Devuelve si se movió.
    fn move_node(&mut self, points: &[[f64; 2]; 5], index: usize, to: [f64; 2]) -> bool {
        if index >= self.node_count() {
            return false;
        }
        if self.can_free(index) {
            let before = self.free[index];
            self.free[index] = Some(to);
            return before != self.free[index];
        }
        if !(1..=self.cuts.len()).contains(&index) {
            return false;
        }
        let low = self.node_t(index - 1) + MIN_GAP;
        let high = self.node_t(index + 1) - MIN_GAP;
        if low > high {
            return false;
        }
        let (t, _) = nearest_t(points, to, low, high, distance);
        let before = self.cuts[index - 1];
        self.cuts[index - 1] = t;
        t != before
    }

    /// Devuelve el nodo `index` a la Bézier, en su `t`.
    fn unfree(&mut self, index: usize) -> bool {
        self.free.get_mut(index).and_then(Option::take).is_some()
    }

    /// Quita el nodo interior `index` y une sus dos tramos: recta si los dos
    /// lo eran.
    fn remove_node(&mut self, index: usize) -> bool {
        if !(1..=self.cuts.len()).contains(&index) {
            return false;
        }
        self.cuts.remove(index - 1);
        self.free.remove(index);
        let merged = self.straight[index - 1] && self.straight[index];
        self.straight.remove(index);
        self.straight[index - 1] = merged;
        self.settle();
        true
    }

    /// Cada tramo como polilínea.
    fn contour(&self, points: &[[f64; 2]; 5]) -> Vec<Vec<[f64; 2]>> {
        (0..self.straight.len())
            .map(|piece| {
                if self.straight[piece] {
                    vec![self.node_at(points, piece), self.node_at(points, piece + 1)]
                } else {
                    let (start, end) = (self.node_t(piece), self.node_t(piece + 1));
                    (0..=SEGMENT_POINTS)
                        .map(|k| {
                            let t = start + (end - start) * k as f64 / SEGMENT_POINTS as f64;
                            point_at(points, t)
                        })
                        .collect()
                }
            })
            .collect()
    }
}

/// Un número con `decimals` decimales, sin ceros de sobra: 0.3 y no 0.300000.
fn trimmed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_string()
}

/// Lee «z mm, r mm» (cualquier unidad de longitud) en mm.
fn node_position(text: &str) -> Option<[f64; 2]> {
    let (z, r) = text.split_once(',')?;
    let mm = |text: &str| {
        hm_units::parse_quantity(text.trim())
            .ok()
            .filter(|q| q.dim() == hm_units::Dimension::LENGTH && q.si.is_finite())
            .map(|q| q.si * 1000.0)
    };
    Some([mm(z)?, mm(r)?])
}

/// Los cortes de las dos tapas.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Segments {
    walls: [WallSegments; 2],
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
fn read(boxes: &Meridian) -> Segments {
    let wall = |texts| WallSegments::from_texts(texts).unwrap_or_default();
    Segments {
        walls: [wall(&boxes.segments.shroud), wall(&boxes.segments.hub)],
    }
}

/// Escribe los cortes en `[meridian]`. Devuelve si cambió algo.
fn write(boxes: &mut Meridian, segments: &Segments) -> bool {
    let shroud = segments.wall(Wall::Shroud).to_texts();
    let hub = segments.wall(Wall::Hub).to_texts();
    let changed = boxes.segments.shroud != shroud || boxes.segments.hub != hub;
    boxes.segments.shroud = shroud;
    boxes.segments.hub = hub;
    changed
}

fn wall_index(wall: Wall) -> usize {
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
        for piece in 0..state.straight.len() {
            let hit = if state.straight[piece] {
                let a = state.node_at(points, piece);
                let b = state.node_at(points, piece + 1);
                let (sa, sb) = (to_screen(plot_ui, a), to_screen(plot_ui, b));
                let f = f64::from(nearest_on_segment(
                    [sa.x, sa.y],
                    [sb.x, sb.y],
                    [screen.x, screen.y],
                ));
                let at = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                (
                    Hit {
                        wall,
                        piece,
                        s: f,
                        at,
                    },
                    metric(at, at),
                )
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

/// Si la tapa tiene algún tramo en recta: entonces la Bézier hidráulica se
/// dibuja de referencia, en gris, y el contorno mecánico encima.
pub(super) fn reference_only(boxes: &Meridian, wall: Wall) -> bool {
    read(boxes).wall(wall).has_straight()
}

/// Dibuja el contorno mecánico de las tapas cortadas, el tramo seleccionado
/// y los puntos: círculos los que se deslizan por la curva, cuadrados los
/// libres.
pub(super) fn draw(plot_ui: &mut egui_plot::PlotUi<'_>, model: &MeridionalModel, boxes: &Meridian) {
    let segments = read(boxes);
    let selected: Option<(Wall, usize)> = plot_ui.ctx().data(|data| data.get_temp(selected_id()));
    for wall in Wall::ALL {
        let state = segments.wall(wall);
        if state.is_plain() && !state.has_straight() {
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
        let (mut sliding, mut free) = (Vec::new(), Vec::new());
        for index in 0..state.node_count() {
            let at = state.node_at(points, index);
            if state.can_free(index) {
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
        let grabbed = origin
            .and_then(|origin| near_node(plot_ui, model, segments, origin, WallSegments::draggable))
            .map(|(wall, index)| Grab {
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
        if state.can_free(grab.index) {
            if ctx.input(|input| input.modifiers.shift) {
                let (dz, dr) = (to[0] - grab.from[0], to[1] - grab.from[1]);
                if dz.abs() >= dr.abs() {
                    to[1] = grab.from[1];
                } else {
                    to[0] = grab.from[0];
                }
            }
            let others: Vec<[f64; 2]> = Wall::ALL
                .into_iter()
                .flat_map(|wall| {
                    let state = segments.wall(wall);
                    let points = &model.params.wall(wall).points;
                    (0..state.node_count())
                        .filter(move |index| (wall, *index) != (grab.wall, grab.index))
                        .map(move |index| state.node_at(points, index))
                        .chain(points.iter().copied())
                        .collect::<Vec<_>>()
                })
                .collect();
            let transform = plot_ui.transform();
            let tolerance = [
                f64::from(SNAP_PIXELS) / transform.dpos_dvalue_x().abs().max(f64::EPSILON),
                f64::from(SNAP_PIXELS) / transform.dpos_dvalue_y().abs().max(f64::EPSILON),
            ];
            let (snapped, guides) = snap(to, &others, tolerance);
            to = snapped;
            if let Some(z) = guides[0] {
                plot_ui.line(line("", vec![[z, -1.0e4], [z, 1.0e4]], SELECTED).width(1.0));
            }
            if let Some(r) = guides[1] {
                plot_ui.line(line("", vec![[-1.0e4, r], [1.0e4, r]], SELECTED).width(1.0));
            }
        }
        changed = segments
            .wall_mut(grab.wall)
            .move_node(&points, grab.index, to);
    }
    let at = segments.wall(grab.wall).node_at(&points, grab.index);
    plot_ui.text(
        egui_plot::Text::new(
            "Punto",
            PlotPoint::new(at[0], at[1]),
            egui::RichText::new(format!("  z = {:.2} mm\n  r = {:.2} mm", at[0], at[1]))
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
/// con «Cortar aquí», «Convertir en recta» y lo demás. Arrastrar un punto lo
/// mueve. Devuelve si cambió algo y si hay un punto agarrado.
pub(super) fn interact(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    boxes: &mut Meridian,
) -> (bool, bool) {
    let ctx = plot_ui.ctx().clone();
    let response = plot_ui.response().clone();
    let seeded = seed_from_env(&ctx, model, boxes);
    let mut segments = read(boxes);
    let (moved, dragging) = drag_node(plot_ui, model, &mut segments);
    if dragging {
        return ((moved && write(boxes, &segments)) || seeded, true);
    }
    if let Some(pointer) = plot_ui.pointer_coordinate() {
        if response.secondary_clicked() {
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
        if response.clicked() {
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
    response.context_menu(|ui| {
        let Some(Target { hit, node }) = target else {
            ui.label("Haz clic derecho sobre una tapa para cortarla.");
            return;
        };
        let points = model.params.wall(hit.wall).points;
        let state = segments.wall_mut(hit.wall);
        ui.label(
            egui::RichText::new(format!(
                "{} · z = {:.2} mm, r = {:.2} mm",
                hit.wall.name(),
                hit.at[0],
                hit.at[1]
            ))
            .small()
            .weak(),
        );
        ui.separator();
        match state.cut_refusal(hit.piece, hit.s) {
            None => {
                if ui.button("Cortar aquí").clicked() {
                    changed |= state.cut(&points, hit.piece, hit.s);
                    ui.close();
                }
            }
            Some(why) => {
                ui.add_enabled(false, egui::Button::new("Cortar aquí"))
                    .on_disabled_hover_text(why);
            }
        }
        if !state.is_plain() {
            let label = if state.straight[hit.piece] {
                "Volver a curva"
            } else {
                "Convertir en recta"
            };
            if ui.button(label).clicked() {
                state.toggle(hit.piece);
                changed = true;
                ui.close();
            }
        }
        if let Some(index) = node {
            if state.free[index].is_some() && ui.button("Devolver el punto a la curva").clicked() {
                changed |= state.unfree(index);
                ui.close();
            }
            if (1..=state.cuts.len()).contains(&index) && ui.button("Quitar este corte").clicked() {
                changed |= state.remove_node(index);
                ui.close();
            }
        }
        if !state.is_plain() && ui.button("Quitar todos los cortes de esta tapa").clicked() {
            *state = WallSegments::default();
            changed = true;
            ui.close();
        }
    });
    if changed {
        // Los índices de los tramos cambian: se olvida la selección.
        ctx.data_mut(|data| data.remove::<(Wall, usize)>(selected_id()));
        return (write(boxes, &segments) || seeded, false);
    }
    (seeded, false)
}

/// Solo para las capturas: `HMF_SEGMENTS="hub:0.5:1:2=60/0"` corta el cubo
/// en `t = 0,5`, pone en recta el tramo 1 y lleva el nodo 2 (el final) a
/// z = 60 mm, r = 0. Cortes, tramos y nodos se separan con `,`; las tapas
/// (`shroud`, `hub`), con `;`.
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
            if piece < state.straight.len() && !state.straight[piece] {
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
        assert_eq!(state.straight, vec![false, true, true, true]);
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

    #[test]
    fn removing_a_cut_merges_the_pieces() {
        let mut state = cut_twice();
        state.toggle(1);
        assert!(state.remove_node(1));
        assert_eq!(state.cuts, vec![0.6]);
        assert_eq!(state.straight, vec![false, false]);
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
        };
        let state = WallSegments::from_texts(&texts).unwrap();
        assert_eq!(state.free, vec![None, None, Some([60.0, 0.0])]);
    }
}
