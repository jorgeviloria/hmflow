//! Las cotas que mandan del contorno meridiano (paso C2): Δz y ΔØ entre dos
//! nodos, de la misma tapa o de las dos. El hijo sigue al padre en una
//! coordenada. Se aplican escribiendo esa coordenada en la posición del hijo,
//! que tiene que ser un punto libre: ninguna cota mueve un nodo sobre la
//! curva.

use eframe::egui;
use egui::Color32;
use egui_plot::PlotPoint;
use hm_design::meridian::{MeridionalModel, Wall};
use hm_project::{Constraint, ConstraintKind, Meridian, NodeRef, WallId};

use crate::curves::line;
use crate::meridian_segments::{
    Segments, WallSegments, mm_value, read, trimmed, wall_index, write,
};

/// Las cotas escritas en `[meridian.segments]`, en su orden.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Constraints {
    items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
enum Item {
    /// Una cota que se entiende, con el texto leído mientras no cambie (así
    /// no se reescribe el `.hmf` solo por leerlo). Las de ángulo se guardan,
    /// pero aún no se aplican.
    Read(Constraint, Option<String>),
    /// Un texto que no se entiende: se conserva tal cual.
    Kept(String),
}

/// Una cota que no se puede cumplir: su índice en la lista y por qué.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Problem {
    pub index: usize,
    pub reason: String,
}

impl Constraints {
    pub(super) fn from_texts(texts: &[String]) -> Self {
        let items = texts
            .iter()
            .map(|text| match Constraint::parse(text) {
                Ok(constraint) => Item::Read(constraint, Some(text.clone())),
                Err(_) => Item::Kept(text.clone()),
            })
            .collect();
        Self { items }
    }

    pub(super) fn to_texts(&self) -> Vec<String> {
        self.items
            .iter()
            .map(|item| match item {
                Item::Read(_, Some(text)) | Item::Kept(text) => text.clone(),
                Item::Read(constraint, None) => constraint.to_text(),
            })
            .collect()
    }

    /// Todas las cotas que se entienden, también las de ángulo.
    fn read_mut(&mut self) -> impl Iterator<Item = (&mut Constraint, &mut Option<String>)> {
        self.items.iter_mut().filter_map(|item| match item {
            Item::Read(constraint, text) => Some((constraint, text)),
            Item::Kept(_) => None,
        })
    }

    fn all(&self) -> impl Iterator<Item = &Constraint> {
        self.items.iter().filter_map(|item| match item {
            Item::Read(constraint, _) => Some(constraint),
            Item::Kept(_) => None,
        })
    }

    /// Por qué no se puede añadir `new`, o `None`.
    pub(super) fn refusal(
        &self,
        walls: &[WallSegments; 2],
        new: &Constraint,
    ) -> Option<&'static str> {
        if new.parent == new.child {
            return Some("padre e hijo son el mismo punto");
        }
        if !walls[wall_index(wall_of(new.child.wall))].is_free_node(new.child.index) {
            return Some("el hijo tiene que ser un punto libre");
        }
        if self
            .all()
            .any(|c| c.child == new.child && c.kind == new.kind)
        {
            return Some("esa cota ya la fija otra");
        }
        if new.kind == ConstraintKind::Ang {
            let (wall, parent, child) = (new.child.wall, new.parent.index, new.child.index);
            if new.parent.wall != wall
                || parent.abs_diff(child) != 1
                || !walls[wall_index(wall_of(wall))].is_line(parent.min(child))
            {
                return Some("el ángulo es de una recta: padre e hijo son sus dos extremos");
            }
            if self.owned(wall_of(wall), child).iter().all(Option::is_some) {
                return Some("el hijo ya tiene z y Ø fijadas: el ángulo sobra");
            }
        }
        if self.reaches(walls, new.parent, new.child) {
            return Some("haría un ciclo");
        }
        None
    }

    /// Si `target` está entre los antepasados de `from`: los padres de sus
    /// cotas y, si va sobre el círculo de una tangencia, los nodos de los que
    /// sale ese círculo. Cada nodo se mira una vez, así que termina aunque el
    /// archivo traiga un ciclo.
    fn reaches(&self, walls: &[WallSegments; 2], from: NodeRef, target: NodeRef) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        let mut ancestors = vec![from];
        while let Some(node) = ancestors.pop() {
            if node == target {
                return true;
            }
            if !seen.insert(node) {
                continue;
            }
            ancestors.extend(self.all().filter(|c| c.child == node).map(|c| c.parent));
            let wall = &walls[wall_index(wall_of(node.wall))];
            ancestors.extend(
                wall.circle_inputs(node.index)
                    .into_iter()
                    .map(|index| NodeRef {
                        wall: node.wall,
                        index,
                    }),
            );
        }
        false
    }

    pub(super) fn add(&mut self, new: Constraint) {
        self.items.push(Item::Read(new, None));
    }

    /// Cambia el valor (en metros) de la cota `index`.
    pub(super) fn set_value(&mut self, index: usize, value: f64) {
        if let Some(Item::Read(constraint, text)) = self.items.get_mut(index) {
            constraint.value = value;
            *text = None;
        }
    }

    pub(super) fn remove(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    /// Se insertó un nodo en `index` de `wall`: los de detrás suben uno.
    pub(super) fn node_inserted(&mut self, wall: Wall, index: usize) {
        let id = id_of(wall);
        for (constraint, text) in self.read_mut() {
            for node in [&mut constraint.parent, &mut constraint.child] {
                if node.wall == id && node.index >= index {
                    node.index += 1;
                    *text = None;
                }
            }
        }
    }

    /// Se quitó el nodo `index` de `wall`: se borran sus cotas y los de detrás
    /// bajan uno. Devuelve cuántas cotas se borraron.
    pub(super) fn node_removed(&mut self, wall: Wall, index: usize) -> usize {
        let gone = NodeRef {
            wall: id_of(wall),
            index,
        };
        let before = self.items.len();
        self.items.retain(
            |item| !matches!(item, Item::Read(c, _) if c.parent == gone || c.child == gone),
        );
        let removed = before - self.items.len();
        for (constraint, text) in self.read_mut() {
            for node in [&mut constraint.parent, &mut constraint.child] {
                if node.wall == gone.wall && node.index > index {
                    node.index -= 1;
                    *text = None;
                }
            }
        }
        removed
    }

    /// Cuántas cotas tocan algún nodo de `wall`.
    pub(super) fn touching_wall(&self, wall: Wall) -> usize {
        let id = id_of(wall);
        self.all()
            .filter(|c| c.parent.wall == id || c.child.wall == id)
            .count()
    }

    /// Quita las cotas que tocan algún nodo de `wall`.
    pub(super) fn remove_wall(&mut self, wall: Wall) {
        let id = id_of(wall);
        self.items.retain(
            |item| !matches!(item, Item::Read(c, _) if c.parent.wall == id || c.child.wall == id),
        );
    }

    /// Cuántas cotas tocan el nodo `index` de `wall`.
    pub(super) fn touching(&self, wall: Wall, index: usize) -> usize {
        let node = NodeRef {
            wall: id_of(wall),
            index,
        };
        self.all()
            .filter(|c| c.parent == node || c.child == node)
            .count()
    }

    /// Si el nodo `index` de `wall` es hijo de alguna cota.
    pub(super) fn has_child(&self, wall: Wall, index: usize) -> bool {
        let node = NodeRef {
            wall: id_of(wall),
            index,
        };
        self.all().any(|c| c.child == node)
    }

    /// Las coordenadas (z, Ø) del nodo `index` de `wall` que fijan una Δz o
    /// una ΔØ, con su padre.
    fn owned(&self, wall: Wall, index: usize) -> [Option<NodeRef>; 2] {
        let node = node_of(wall, index);
        let mut owned = [None; 2];
        for constraint in self.all().filter(|c| c.child == node) {
            match constraint.kind {
                ConstraintKind::Dz => owned[0] = Some(constraint.parent),
                ConstraintKind::Dd => owned[1] = Some(constraint.parent),
                ConstraintKind::Ang => {}
            }
        }
        owned
    }

    /// El padre y el tipo de la cota que fija cada coordenada (z, Ø) del nodo
    /// `index` de `wall`. Un ángulo fija la que deriva de la otra.
    pub(super) fn fixing(
        &self,
        wall: Wall,
        index: usize,
    ) -> [Option<(NodeRef, ConstraintKind)>; 2] {
        let owned = self.owned(wall, index);
        let mut fixed = [
            owned[0].map(|parent| (parent, ConstraintKind::Dz)),
            owned[1].map(|parent| (parent, ConstraintKind::Dd)),
        ];
        let node = node_of(wall, index);
        for constraint in self
            .all()
            .filter(|c| c.child == node && c.kind == ConstraintKind::Ang)
        {
            let flags = [owned[0].is_some(), owned[1].is_some()];
            if let Some(axis) = derived_axis(constraint.value, flags) {
                fixed[axis] = Some((constraint.parent, ConstraintKind::Ang));
            }
        }
        fixed
    }

    /// Si la recta `piece` de `wall` tiene un ángulo que manda.
    pub(super) fn has_angle(&self, wall: Wall, piece: usize) -> bool {
        let id = id_of(wall);
        self.all().any(|c| {
            c.kind == ConstraintKind::Ang
                && c.parent.wall == id
                && c.child.wall == id
                && c.parent.index.min(c.child.index) == piece
        })
    }

    /// Quita los ángulos de la recta `piece` de `wall`, que ya no lo es.
    /// Devuelve cuántos quitó.
    pub(super) fn line_lost(&mut self, wall: Wall, piece: usize) -> usize {
        let id = id_of(wall);
        let before = self.items.len();
        self.items.retain(|item| {
            !matches!(item, Item::Read(c, _) if c.kind == ConstraintKind::Ang
                && c.parent.wall == id
                && c.child.wall == id
                && c.parent.index.min(c.child.index) == piece)
        });
        before - self.items.len()
    }

    /// Las cotas que se entienden, con su índice en la lista.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &Constraint)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                Item::Read(constraint, _) => Some((index, constraint)),
                Item::Kept(_) => None,
            })
    }
}

/// El valor de una cota como se escribe: mm para Δz y ΔØ, grados para el
/// ángulo.
pub(super) fn shown(constraint: &Constraint) -> f64 {
    if constraint.kind == ConstraintKind::Ang {
        constraint.value
    } else {
        constraint.value * 1000.0
    }
}

/// Lo contrario de `shown`: el valor guardado de lo escrito. El ángulo se
/// lleva a (−180, 180].
fn stored(kind: ConstraintKind, typed: f64) -> f64 {
    if kind == ConstraintKind::Ang {
        let mut angle = typed % 360.0;
        if angle > 180.0 {
            angle -= 360.0;
        } else if angle <= -180.0 {
            angle += 360.0;
        }
        angle
    } else {
        typed / 1000.0
    }
}

/// La coordenada (0 z, 1 Ø) que deriva un ángulo `value` (grados) del hijo, o
/// `None` si no hay ninguna libre. `owned` dice cuáles manda ya una Δz o una
/// ΔØ. Sin ellas se conserva la que menos cambia: con menos de 45° contra el
/// eje, la z.
fn derived_axis(value: f64, owned: [bool; 2]) -> Option<usize> {
    match owned {
        [true, true] => None,
        [true, false] => Some(1),
        [false, true] => Some(0),
        [false, false] => {
            let angle = value.abs();
            let acute = if angle > 90.0 { 180.0 - angle } else { angle };
            Some(usize::from(acute < 45.0))
        }
    }
}

/// `to` con las coordenadas que manda una cota en su valor de `at`: el
/// arrastre de un hijo solo recorre la que queda libre.
pub(super) fn lock_fixed<T>(fixed: [Option<T>; 2], at: [f64; 2], mut to: [f64; 2]) -> [f64; 2] {
    for axis in 0..2 {
        if fixed[axis].is_some() {
            to[axis] = at[axis];
        }
    }
    to
}

/// Por qué una coordenada no se escribe: la fija una cota desde `parent`.
pub(super) fn fixed_reason(parent: NodeRef, kind: ConstraintKind) -> String {
    let kind = match kind {
        ConstraintKind::Dz => "Δz",
        ConstraintKind::Dd => "ΔØ",
        ConstraintKind::Ang => "el ángulo",
    };
    let wall = match parent.wall {
        WallId::Hub => "hub",
        WallId::Shroud => "shroud",
    };
    format!("la fija {kind} desde {wall}:{}", parent.index)
}

pub(super) fn wall_of(id: WallId) -> Wall {
    match id {
        WallId::Shroud => Wall::Shroud,
        WallId::Hub => Wall::Hub,
    }
}

pub(super) fn id_of(wall: Wall) -> WallId {
    match wall {
        Wall::Shroud => WallId::Shroud,
        Wall::Hub => WallId::Hub,
    }
}

/// Los puntos de control de las dos tapas, en el orden de `wall_index`.
fn wall_points(model: &MeridionalModel) -> [&[[f64; 2]; 5]; 2] {
    let mut points = [&model.params.wall(Wall::Shroud).points; 2];
    points[wall_index(Wall::Hub)] = &model.params.wall(Wall::Hub).points;
    points
}

/// Cumple las cotas sobre lo escrito en `[meridian]`. Devuelve si cambió algo.
/// Sin cotas no toca nada, para no reescribir un `.hmf` solo por leerlo.
pub(super) fn enforce(model: &MeridionalModel, boxes: &mut Meridian) -> bool {
    enforce_with(wall_points(model), boxes)
}

fn enforce_with(points: [&[[f64; 2]; 5]; 2], boxes: &mut Meridian) -> bool {
    if boxes.segments.constraints.is_empty() {
        return false;
    }
    // Una tapa que no se entiende se leería como entera y `write` la
    // reescribiría: se deja como está.
    if WallSegments::from_texts(&boxes.segments.shroud).is_none()
        || WallSegments::from_texts(&boxes.segments.hub).is_none()
    {
        return false;
    }
    let mut segments = read(boxes);
    let Segments {
        walls, constraints, ..
    } = &mut segments;
    apply(constraints, points, walls);
    write(boxes, &segments)
}

/// Gris oscuro de las cotas que mandan; las azules de C1 son medidas.
const COMMANDING: Color32 = Color32::from_gray(40);
const BROKEN: Color32 = Color32::from_rgb(200, 30, 30);

/// Qué es un valor dibujado: una cota que manda, o una provisional del par
/// que se está acotando.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Existing(usize),
    New(ConstraintKind),
}

fn values_id() -> egui::Id {
    egui::Id::unique("meridian_constraints_values")
}

fn pending_id() -> egui::Id {
    egui::Id::unique("meridian_constraints_pending")
}

fn notice_id() -> egui::Id {
    egui::Id::unique("meridian_constraints_notice")
}

fn writing_id() -> egui::Id {
    egui::Id::unique("meridian_constraints_writing")
}

fn menu_id() -> egui::Id {
    egui::Id::unique("meridian_constraints_menu")
}

fn node_of(wall: Wall, index: usize) -> NodeRef {
    NodeRef {
        wall: id_of(wall),
        index,
    }
}

/// Dónde está un nodo, si existe.
fn position(
    walls: &[WallSegments; 2],
    points: [&[[f64; 2]; 5]; 2],
    node: NodeRef,
) -> Option<[f64; 2]> {
    let wall = wall_index(wall_of(node.wall));
    (node.index < walls[wall].node_count()).then(|| walls[wall].node_at(points[wall], node.index))
}

/// Los dos extremos de la línea de una cota del hijo `to` desde `from`:
/// horizontal a la r del hijo para Δz, vertical a la z del hijo para ΔØ.
fn ends(kind: ConstraintKind, from: [f64; 2], to: [f64; 2]) -> [[f64; 2]; 2] {
    match kind {
        ConstraintKind::Dd => [[to[0], from[1]], to],
        _ => [[from[0], to[1]], to],
    }
}

/// Dibuja una cota y devuelve el rectángulo en pantalla de su valor.
fn dimension(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    ends: [[f64; 2]; 2],
    text: String,
    colour: Color32,
) -> egui::Rect {
    plot_ui.line(line("Cota que manda", ends.to_vec(), colour).width(1.0));
    label_between(plot_ui, ends, text, colour)
}

/// El texto de una cota a mitad de `ends`, sin línea. Devuelve su rectángulo
/// en pantalla.
fn label_between(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    ends: [[f64; 2]; 2],
    text: String,
    colour: Color32,
) -> egui::Rect {
    let middle = PlotPoint::new(
        (ends[0][0] + ends[1][0]) / 2.0,
        (ends[0][1] + ends[1][1]) / 2.0,
    );
    let width = 7.0 * text.chars().count() as f32;
    plot_ui.text(
        egui_plot::Text::new(
            "Cota que manda",
            middle,
            egui::RichText::new(text).small().color(colour),
        )
        .anchor(egui::Align2::CENTER_BOTTOM),
    );
    let screen = plot_ui.screen_from_plot(middle);
    egui::Rect::from_center_size(screen - egui::vec2(0.0, 7.0), egui::vec2(width, 14.0))
}

/// El ángulo agudo que hace una recta de dirección `value` (grados) con el eje.
fn acute(value: f64) -> f64 {
    let angle = value.abs();
    if angle > 90.0 { 180.0 - angle } else { angle }
}

fn mm_text(value: f64) -> String {
    trimmed(value, 2).replace('-', "−")
}

/// Dibuja las cotas que mandan (en rojo y con el motivo las que no se
/// cumplen) y, si el punto abierto (`open`) es el hijo de un par que se está
/// acotando, sus dos cotas provisionales en gris. Guarda dónde queda cada
/// valor, para el clic.
pub(super) fn draw(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &Segments,
    open: Option<(Wall, usize)>,
) {
    let ctx = plot_ui.ctx().clone();
    let points = wall_points(model);
    let mut walls = segments.walls.clone();
    let problems = apply(&segments.constraints, points, &mut walls);
    let mut values = Vec::new();
    for (index, constraint) in segments.constraints.iter() {
        let (Some(from), Some(to)) = (
            position(&walls, points, constraint.parent),
            position(&walls, points, constraint.child),
        ) else {
            continue;
        };
        let label = match constraint.kind {
            ConstraintKind::Dz => format!("Δz {}", mm_text(shown(constraint))),
            ConstraintKind::Dd => format!("ΔØ {}", mm_text(shown(constraint))),
            // En pantalla, el ángulo agudo con el eje; el signo se guarda.
            ConstraintKind::Ang => format!("{}°", trimmed(acute(constraint.value), 1)),
        };
        let (colour, text) = match problems.iter().find(|problem| problem.index == index) {
            Some(problem) => (BROKEN, format!("{label} · {}", problem.reason)),
            None => (COMMANDING, label),
        };
        let rect = if constraint.kind == ConstraintKind::Ang {
            label_between(plot_ui, [from, to], text, colour)
        } else {
            dimension(plot_ui, ends(constraint.kind, from, to), text, colour)
        };
        values.push((rect, Value::Existing(index)));
    }
    // Junto a cada hijo: qué tiene fijado.
    let mut children: Vec<NodeRef> = segments.constraints.iter().map(|(_, c)| c.child).collect();
    children.sort();
    children.dedup();
    for child in children {
        let Some(at) = position(&walls, points, child) else {
            continue;
        };
        let fixed = segments
            .constraints
            .fixing(wall_of(child.wall), child.index)
            .map(|axis| axis.is_some());
        let mark = match fixed {
            [true, true] => "[z Ø]",
            [true, false] => "[z]",
            [false, true] => "[Ø]",
            [false, false] => continue,
        };
        plot_ui.text(
            egui_plot::Text::new(
                "Cota que manda",
                PlotPoint::new(at[0], at[1]),
                egui::RichText::new(format!(" {mark}"))
                    .small()
                    .color(COMMANDING),
            )
            .anchor(egui::Align2::LEFT_BOTTOM),
        );
    }
    let open = open.map(|(wall, index)| node_of(wall, index));
    let pending: Option<Pending> = ctx.data(|data| data.get_temp(pending_id())).flatten();
    match pending {
        Some(pending) if Some(pending.child) == open => {
            if let (Some(from), Some(to)) = (
                position(&walls, points, pending.parent),
                position(&walls, points, pending.child),
            ) {
                let (dz, dd) = measured(&walls, points, &pending);
                for (kind, symbol, value) in [
                    (ConstraintKind::Dz, "Δz", dz),
                    (ConstraintKind::Dd, "ΔØ", dd),
                ] {
                    let text = format!("{symbol} {}?", mm_text(value));
                    let rect = dimension(plot_ui, ends(kind, from, to), text, Color32::GRAY);
                    values.push((rect, Value::New(kind)));
                }
            }
        }
        Some(_) => {
            ctx.data_mut(|data| data.insert_temp::<Option<Pending>>(pending_id(), None));
        }
        None => {}
    }
    let notice: Option<(NodeRef, String)> = ctx.data(|data| data.get_temp(notice_id())).flatten();
    if let Some((node, text)) = notice {
        match position(&walls, points, node) {
            Some(at) if Some(node) == open => {
                plot_ui.text(
                    egui_plot::Text::new(
                        "Cota que manda",
                        PlotPoint::new(at[0], at[1]),
                        egui::RichText::new(format!("  {text}"))
                            .small()
                            .color(BROKEN),
                    )
                    .anchor(egui::Align2::LEFT_TOP),
                );
            }
            _ => ctx.data_mut(|data| {
                data.insert_temp::<Option<(NodeRef, String)>>(notice_id(), None);
            }),
        }
    }
    ctx.data_mut(|data| data.insert_temp(values_id(), values));
}

/// Mayús+clic en `parent` con la ventana de `child` abierta: empieza a acotar
/// el par, o dice por qué no se puede.
pub(super) fn pick_parent(
    ctx: &egui::Context,
    segments: &Segments,
    child: (Wall, usize),
    parent: (Wall, usize),
) {
    let (child, parent) = (node_of(child.0, child.1), node_of(parent.0, parent.1));
    let free = segments.walls[wall_index(wall_of(child.wall))].is_free_node(child.index);
    ctx.data_mut(|data| {
        if free {
            data.insert_temp(pending_id(), Some(Pending { child, parent }));
            data.insert_temp::<Option<(NodeRef, String)>>(notice_id(), None);
        } else {
            data.insert_temp::<Option<Pending>>(pending_id(), None);
            data.insert_temp(
                notice_id(),
                Some((child, "el hijo tiene que ser un punto libre".to_string())),
            );
        }
    });
}

/// El par (hijo, padre) de un ángulo sobre la recta `piece` de `wall`: el
/// extremo libre es el hijo; si lo son los dos, el más cercano al clic
/// (`s`, de 0 a 1 a lo largo del tramo). O por qué no se puede.
pub(super) fn angle_pair(
    walls: &[WallSegments; 2],
    constraints: &Constraints,
    wall: Wall,
    piece: usize,
    s: f64,
) -> Result<Pending, &'static str> {
    let state = &walls[wall_index(wall)];
    if !state.is_line(piece) {
        return Err("el ángulo es de una recta");
    }
    let (start, end) = (node_of(wall, piece), node_of(wall, piece + 1));
    let pair = |child, parent| Pending { child, parent };
    let candidates = [pair(start, end), pair(end, start)];
    let probe = |pending: &Pending| {
        let new = Constraint {
            kind: ConstraintKind::Ang,
            parent: pending.parent,
            child: pending.child,
            value: 0.0,
        };
        constraints.refusal(walls, &new)
    };
    let mut allowed: Vec<Pending> = candidates
        .into_iter()
        .filter(|pending| probe(pending).is_none())
        .collect();
    match allowed.len() {
        0 => Err(probe(&candidates[0]).unwrap_or("no se puede fijar el ángulo")),
        1 => Ok(allowed.remove(0)),
        _ => Ok(if s < 0.5 {
            candidates[0]
        } else {
            candidates[1]
        }),
    }
}

/// Abre la caja del ángulo de la recta `piece` con el que tiene hoy.
pub(super) fn start_angle(
    ctx: &egui::Context,
    model: &MeridionalModel,
    walls: &[WallSegments; 2],
    pending: Pending,
) {
    let points = wall_points(model);
    let (Some(from), Some(to), Some(screen)) = (
        position(walls, points, pending.parent),
        position(walls, points, pending.child),
        ctx.input(|input| input.pointer.interact_pos()),
    ) else {
        return;
    };
    let angle = (to[1] - from[1]).atan2(to[0] - from[0]).to_degrees();
    let writing = Writing {
        value: Value::New(ConstraintKind::Ang),
        pending: Some(pending),
        text: trimmed(stored(ConstraintKind::Ang, angle), 1),
        screen,
        fresh: true,
        refusal: None,
    };
    ctx.data_mut(|data| data.insert_temp(writing_id(), Some(writing)));
}

/// Se intentó arrastrar un hijo con z y Ø fijadas: el aviso junto a su punto.
pub(super) fn refuse_drag(ctx: &egui::Context, wall: Wall, index: usize) {
    ctx.data_mut(|data| {
        data.insert_temp(
            notice_id(),
            Some((
                node_of(wall, index),
                "z y Ø las fijan cotas: bórralas para moverlo".to_string(),
            )),
        );
    });
}

/// El valor bajo el puntero, si lo hay.
fn value_under(ctx: &egui::Context) -> Option<Value> {
    let pointer = ctx.input(|input| input.pointer.interact_pos())?;
    let values: Vec<(egui::Rect, Value)> = ctx.data(|data| data.get_temp(values_id()))?;
    values
        .into_iter()
        .find(|(rect, _)| rect.contains(pointer))
        .map(|(_, value)| value)
}

/// Una cota que se está escribiendo.
#[derive(Clone, Debug)]
struct Writing {
    value: Value,
    pending: Option<Pending>,
    text: String,
    screen: egui::Pos2,
    fresh: bool,
    refusal: Option<&'static str>,
}

/// Clic en el valor de una cota (que manda o provisional): se abre para
/// escribirlo. Devuelve si el clic cayó en uno.
pub(super) fn click(ctx: &egui::Context, model: &MeridionalModel, segments: &Segments) -> bool {
    let Some(value) = value_under(ctx) else {
        return false;
    };
    let points = wall_points(model);
    let pending: Option<Pending> = ctx.data(|data| data.get_temp(pending_id())).flatten();
    let text = match value {
        Value::Existing(index) => segments
            .constraints
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, c)| trimmed(shown(c), 2)),
        Value::New(kind) => pending.map(|pending| {
            let (dz, dd) = measured(&segments.walls, points, &pending);
            trimmed(if kind == ConstraintKind::Dz { dz } else { dd }, 2)
        }),
    };
    let (Some(text), Some(screen)) = (text, ctx.input(|input| input.pointer.interact_pos())) else {
        return false;
    };
    let writing = Writing {
        value,
        pending,
        text,
        screen,
        fresh: true,
        refusal: None,
    };
    ctx.data_mut(|data| data.insert_temp(writing_id(), Some(writing)));
    true
}

/// Clic derecho: recuerda la cota que manda bajo el puntero, para el menú.
pub(super) fn remember_menu(ctx: &egui::Context) {
    let index = match value_under(ctx) {
        Some(Value::Existing(index)) => Some(index),
        _ => None,
    };
    ctx.data_mut(|data| data.insert_temp(menu_id(), index));
}

/// La cota que manda sobre la que se abrió el menú.
pub(super) fn menu_constraint(ctx: &egui::Context) -> Option<usize> {
    ctx.data(|data| data.get_temp(menu_id())).flatten()
}

/// La caja de la cota que se escribe. Intro la crea o la cambia; Esc o un
/// clic fuera la cierran. Devuelve si cambió algo.
pub(super) fn writing(ctx: &egui::Context, segments: &mut Segments) -> bool {
    let Some(mut writing) = ctx
        .data(|data| data.get_temp::<Option<Writing>>(writing_id()))
        .flatten()
    else {
        return false;
    };
    let field_id = writing_id().with("field");
    let (mut done, mut apply) = (false, false);
    egui::Area::new(writing_id().with("area"))
        .order(egui::Order::Foreground)
        .fixed_pos(writing.screen - egui::vec2(50.0, 14.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let kind = match writing.value {
                        Value::New(kind) => Some(kind),
                        Value::Existing(index) => segments
                            .constraints
                            .iter()
                            .find(|(i, _)| *i == index)
                            .map(|(_, c)| c.kind),
                    };
                    ui.label(match kind {
                        Some(ConstraintKind::Dz) => "Δz",
                        Some(ConstraintKind::Ang) => "∠",
                        _ => "ΔØ",
                    });
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut writing.text)
                            .id(field_id)
                            .desired_width(72.0),
                    );
                    ui.label(if kind == Some(ConstraintKind::Ang) {
                        "°"
                    } else {
                        "mm"
                    });
                    if writing.fresh {
                        field.request_focus();
                    } else if field.lost_focus() {
                        apply = ui.input(|input| input.key_pressed(egui::Key::Enter));
                        done = true;
                    }
                });
                if let Some(refusal) = writing.refusal {
                    ui.label(egui::RichText::new(refusal).small().color(BROKEN));
                }
            });
        });
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        done = true;
        apply = false;
    }
    writing.fresh = false;
    let mut changed = false;
    if apply {
        match commit(segments, writing.value, writing.pending, &writing.text) {
            Ok(done_changed) => {
                changed = done_changed;
                if matches!(writing.value, Value::New(_)) {
                    ctx.data_mut(|data| data.insert_temp::<Option<Pending>>(pending_id(), None));
                }
            }
            Err(refusal) => {
                // Sigue abierta, con el motivo.
                writing.refusal = Some(refusal);
                writing.fresh = true;
                done = false;
            }
        }
    }
    ctx.data_mut(|data| data.insert_temp(writing_id(), (!done).then_some(writing)));
    changed
}

/// Aplica lo escrito en la caja: cambia el valor de una cota que manda o crea
/// la del par que se está acotando. Devuelve si cambió algo, o por qué no se
/// pudo.
fn commit(
    segments: &mut Segments,
    value: Value,
    pending: Option<Pending>,
    text: &str,
) -> Result<bool, &'static str> {
    let mm = mm_value(text).ok_or("no es un número")?;
    match (value, pending) {
        (Value::Existing(index), _) => {
            let same = segments
                .constraints
                .iter()
                .find(|(i, _)| *i == index)
                .is_some_and(|(_, c)| (shown(c) - mm).abs() < 1e-9);
            if same {
                return Ok(false);
            }
            let kind = segments.constraints.iter().find(|(i, _)| *i == index);
            if let Some((_, c)) = kind {
                let value = stored(c.kind, mm);
                segments.constraints.set_value(index, value);
            }
            Ok(true)
        }
        (Value::New(kind), Some(pending)) => {
            let new = Constraint {
                kind,
                parent: pending.parent,
                child: pending.child,
                value: stored(kind, mm),
            };
            match segments.constraints.refusal(&segments.walls, &new) {
                None => {
                    segments.constraints.add(new);
                    Ok(true)
                }
                Some(refusal) => Err(refusal),
            }
        }
        (Value::New(_), None) => Ok(false),
    }
}

/// Se cortó o se quitó un punto: los números del par a medio acotar y de la
/// caja abierta ya no valen, así que se olvidan.
pub(super) fn forget_unfinished(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.insert_temp::<Option<Pending>>(pending_id(), None);
        data.insert_temp::<Option<Writing>>(writing_id(), None);
        data.insert_temp::<Option<(NodeRef, String)>>(notice_id(), None);
    });
}

/// Un par que se está acotando: el hijo (el punto de la ventana abierta) y el
/// padre (Mayús+clic).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pending {
    pub child: NodeRef,
    pub parent: NodeRef,
}

/// La medida de hoy entre el padre y el hijo, en mm: (Δz, ΔØ).
fn measured(
    walls: &[WallSegments; 2],
    points: [&[[f64; 2]; 5]; 2],
    pending: &Pending,
) -> (f64, f64) {
    match (
        position(walls, points, pending.parent),
        position(walls, points, pending.child),
    ) {
        (Some(p), Some(c)) => (c[0] - p[0], 2.0 * (c[1] - p[1])),
        _ => (0.0, 0.0),
    }
}

/// «hub:2» o «shroud:0».
fn node_ref(text: &str) -> Option<NodeRef> {
    let (wall, index) = text.split_once(':')?;
    let wall = match wall {
        "hub" => WallId::Hub,
        "shroud" => WallId::Shroud,
        _ => return None,
    };
    Some(NodeRef {
        wall,
        index: index.parse().ok()?,
    })
}

/// Solo para las capturas: `HMF_CONSTRAINTS="dz hub:1 hub:2 = 40 mm;dd …"`
/// escribe esas cotas, una vez, como si vinieran del `.hmf`.
pub(super) fn seed_from_env(ctx: &egui::Context, boxes: &mut Meridian) -> bool {
    let seeded = egui::Id::unique("meridian_constraints_seeded");
    if ctx.data(|data| data.get_temp::<bool>(seeded)).is_some() {
        return false;
    }
    ctx.data_mut(|data| data.insert_temp(seeded, true));
    let Ok(text) = std::env::var("HMF_CONSTRAINTS") else {
        return false;
    };
    let mut constraints = Vec::new();
    for part in text
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        // `pending <padre> <hijo>` deja el par a medio acotar.
        if let Some(rest) = part.strip_prefix("pending") {
            let nodes: Vec<NodeRef> = rest.split_whitespace().filter_map(node_ref).collect();
            if let [parent, child] = nodes[..] {
                ctx.data_mut(|data| {
                    data.insert_temp(pending_id(), Some(Pending { child, parent }))
                });
            }
        } else {
            constraints.push(part.to_string());
        }
    }
    boxes.segments.constraints = constraints;
    true
}

/// Aplica las cotas, padres antes que hijos, sobre las dos tapas (en el orden
/// de `wall_index`). Devuelve las que no se pudieron cumplir; su hijo se
/// queda donde estaba.
pub(super) fn apply(
    constraints: &Constraints,
    points: [&[[f64; 2]; 5]; 2],
    walls: &mut [WallSegments; 2],
) -> Vec<Problem> {
    let mut pending: Vec<(usize, Constraint)> =
        constraints.iter().map(|(index, c)| (index, *c)).collect();
    let mut problems = Vec::new();
    // Orden topológico: cada vez, las cotas cuyo padre ya no espera a otra.
    while !pending.is_empty() {
        let ready: Vec<(usize, Constraint)> = pending
            .iter()
            .filter(|(_, c)| {
                !pending.iter().any(|(_, other)| other.child == c.parent)
                    // El ángulo se cumple tras las Δz y ΔØ de su hijo.
                    && !(c.kind == ConstraintKind::Ang
                        && pending
                            .iter()
                            .any(|(_, o)| o.child == c.child && o.kind != ConstraintKind::Ang))
            })
            .copied()
            .collect();
        if ready.is_empty() {
            problems.extend(pending.drain(..).map(|(index, _)| Problem {
                index,
                reason: "forma un ciclo".into(),
            }));
            break;
        }
        pending.retain(|item| !ready.contains(item));
        for (index, constraint) in ready {
            // Un ciclo escondido en un círculo tangente no se cumple nunca.
            let result = if constraints.reaches(walls, constraint.parent, constraint.child) {
                Err("forma un ciclo".into())
            } else {
                apply_one(&constraint, constraints, points, walls)
            };
            if let Err(reason) = result {
                problems.push(Problem { index, reason });
            }
        }
    }
    problems.sort_by_key(|problem| problem.index);
    problems
}

/// Aplica una cota, o dice por qué no se puede.
fn apply_one(
    constraint: &Constraint,
    constraints: &Constraints,
    points: [&[[f64; 2]; 5]; 2],
    walls: &mut [WallSegments; 2],
) -> Result<(), String> {
    let place = |node: NodeRef| (wall_index(wall_of(node.wall)), node.index);
    let ((parent_wall, parent), (child_wall, child)) =
        (place(constraint.parent), place(constraint.child));
    if parent >= walls[parent_wall].node_count() || child >= walls[child_wall].node_count() {
        return Err("el punto no existe".into());
    }
    if !walls[child_wall].is_free_node(child) {
        return Err("el hijo está sobre la curva".into());
    }
    let from = walls[parent_wall].node_at(points[parent_wall], parent);
    let (axis, value) = match constraint.kind {
        ConstraintKind::Dz => (0, from[0] + constraint.value * 1000.0),
        ConstraintKind::Dd => (1, from[1] + constraint.value * 500.0),
        ConstraintKind::Ang => {
            let owned = constraints.owned(wall_of(constraint.child.wall), child);
            let flags = [owned[0].is_some(), owned[1].is_some()];
            let Some(axis) = derived_axis(constraint.value, flags) else {
                return Ok(());
            };
            let at = walls[child_wall].node_at(points[child_wall], child);
            let (sin, cos) = constraint.value.to_radians().sin_cos();
            let value = if axis == 1 {
                if cos.abs() < 1e-9 {
                    return Err("con esa z fijada una recta vertical no cabe".into());
                }
                from[1] + (at[0] - from[0]) * sin / cos
            } else {
                if sin.abs() < 1e-9 {
                    return Err("con ese Ø fijado una recta horizontal no cabe".into());
                }
                from[0] + (at[1] - from[1]) * cos / sin
            };
            (axis, value)
        }
    };
    if axis == 1 && value < 0.0 {
        let diameter = trimmed(2.0 * value, 1).replace('-', "−");
        return Err(format!("Ø {diameter} mm no cabe"));
    }
    walls[child_wall].set_free_axis(points[child_wall], child, axis, value);
    Ok(())
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

    /// Cubo con dos cortes y tres rectas: los cuatro nodos libres.
    fn three_lines() -> WallSegments {
        let mut state = WallSegments::default();
        assert!(state.cut_at_t(&POINTS, 0.3));
        assert!(state.cut_at_t(&POINTS, 0.6));
        for piece in 0..3 {
            state.toggle(piece);
        }
        state
    }

    /// Curva, recta, recta: los nodos 0 y 1 sobre la curva, 2 y 3 libres.
    fn curve_then_lines() -> WallSegments {
        let mut state = three_lines();
        state.toggle(0);
        state
    }

    fn walls(hub: WallSegments) -> [WallSegments; 2] {
        let mut walls: [WallSegments; 2] = Default::default();
        walls[wall_index(Wall::Hub)] = hub;
        walls
    }

    fn with(texts: &[&str]) -> Constraints {
        Constraints::from_texts(&texts.iter().map(|t| (*t).to_string()).collect::<Vec<_>>())
    }

    fn at(walls: &[WallSegments; 2], wall: Wall, index: usize) -> [f64; 2] {
        walls[wall_index(wall)].node_at(&POINTS, index)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn fixing_tells_which_coordinates_a_child_has_fixed() {
        let constraints = with(&["dz hub:1 hub:2 = 15 mm", "dd hub:0 hub:2 = 5 mm"]);
        let parent = |index, kind| {
            Some((
                NodeRef {
                    wall: WallId::Hub,
                    index,
                },
                kind,
            ))
        };
        assert_eq!(
            constraints.fixing(Wall::Hub, 2),
            [parent(1, ConstraintKind::Dz), parent(0, ConstraintKind::Dd)]
        );
        assert_eq!(constraints.fixing(Wall::Hub, 1), [None, None]);
        assert_eq!(constraints.fixing(Wall::Shroud, 2), [None, None]);
    }

    #[test]
    fn a_dragged_child_only_moves_along_the_free_coordinate() {
        let constraints = with(&["dz hub:1 hub:2 = 15 mm"]);
        let fixed = constraints.fixing(Wall::Hub, 2);
        assert_eq!(lock_fixed(fixed, [10.0, 4.0], [20.0, 9.0]), [10.0, 9.0]);
        let none = constraints.fixing(Wall::Hub, 1);
        assert_eq!(lock_fixed(none, [10.0, 4.0], [20.0, 9.0]), [20.0, 9.0]);
    }

    #[test]
    fn the_reason_names_the_kind_and_the_parent() {
        let parent = NodeRef {
            wall: WallId::Shroud,
            index: 4,
        };
        assert_eq!(
            fixed_reason(parent, ConstraintKind::Dz),
            "la fija Δz desde shroud:4"
        );
        assert_eq!(
            fixed_reason(parent, ConstraintKind::Dd),
            "la fija ΔØ desde shroud:4"
        );
        assert_eq!(
            fixed_reason(parent, ConstraintKind::Ang),
            "la fija el ángulo desde shroud:4"
        );
    }

    fn angle_of(walls: &[WallSegments; 2], from: usize, to: usize) -> f64 {
        let (a, b) = (at(walls, Wall::Hub, from), at(walls, Wall::Hub, to));
        (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees()
    }

    #[test]
    fn a_shallow_angle_keeps_z_and_derives_r() {
        let mut walls = walls(three_lines());
        let z_before = at(&walls, Wall::Hub, 2)[0];
        let problems = apply(
            &with(&["ang hub:1 hub:2 = 30 deg"]),
            [&POINTS; 2],
            &mut walls,
        );
        assert!(problems.is_empty());
        assert!(close(at(&walls, Wall::Hub, 2)[0], z_before));
        assert!((angle_of(&walls, 1, 2) - 30.0).abs() < 1e-6);
    }

    #[test]
    fn a_steep_angle_keeps_r_and_derives_z() {
        let mut walls = walls(three_lines());
        let r_before = at(&walls, Wall::Hub, 2)[1];
        let problems = apply(
            &with(&["ang hub:1 hub:2 = -120 deg"]),
            [&POINTS; 2],
            &mut walls,
        );
        assert!(problems.is_empty());
        assert!(close(at(&walls, Wall::Hub, 2)[1], r_before));
        // Conservando una coordenada el hijo queda sobre la recta, a un lado u
        // otro del padre: el ángulo se cumple módulo 180°.
        assert!(
            (angle_of(&walls, 1, 2) + 120.0)
                .rem_euclid(180.0)
                .min(180.0 - (angle_of(&walls, 1, 2) + 120.0).rem_euclid(180.0))
                < 1e-6
        );
    }

    #[test]
    fn a_dz_and_an_angle_leave_the_child_complete() {
        let mut walls = walls(three_lines());
        // El ángulo va primero en el archivo: se cumple después igualmente.
        let constraints = with(&["ang hub:1 hub:2 = 80 deg", "dz hub:0 hub:2 = 40 mm"]);
        let problems = apply(&constraints, [&POINTS; 2], &mut walls);
        assert!(problems.is_empty());
        let z0 = at(&walls, Wall::Hub, 0)[0];
        assert!(close(at(&walls, Wall::Hub, 2)[0], z0 + 40.0));
        assert!((angle_of(&walls, 1, 2) - 80.0).abs() < 1e-6);
        let fixed = constraints.fixing(Wall::Hub, 2);
        assert_eq!(fixed[0].map(|f| f.1), Some(ConstraintKind::Dz));
        assert_eq!(fixed[1].map(|f| f.1), Some(ConstraintKind::Ang));
    }

    #[test]
    fn an_angle_alone_fixes_the_derived_coordinate() {
        let constraints = with(&["ang hub:1 hub:2 = 30 deg"]);
        let fixed = constraints.fixing(Wall::Hub, 2);
        assert!(fixed[0].is_none());
        assert_eq!(fixed[1].map(|f| f.1), Some(ConstraintKind::Ang));
    }

    #[test]
    fn a_vertical_line_cannot_keep_a_fixed_z() {
        let mut walls = walls(three_lines());
        let constraints = with(&["dz hub:0 hub:2 = 40 mm", "ang hub:1 hub:2 = 90 deg"]);
        let problems = apply(&constraints, [&POINTS; 2], &mut walls);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].index, 1);
    }

    #[test]
    fn an_angle_is_refused_off_a_line_or_when_the_child_is_complete() {
        let walls = walls(curve_then_lines());
        let refusal = |text: &str| with(&[]).refusal(&walls, &parsed(text));
        assert!(refusal("ang hub:2 hub:3 = 30 deg").is_none());
        // La curva (tramo 0) no tiene ángulo; tampoco nodos que no son vecinos.
        assert!(refusal("ang hub:0 hub:1 = 30 deg").is_some());
        assert!(refusal("ang hub:1 hub:3 = 30 deg").is_some());
        let both = with(&["dz hub:1 hub:3 = 5 mm", "dd hub:1 hub:3 = 5 mm"]);
        assert!(
            both.refusal(&walls, &parsed("ang hub:2 hub:3 = 30 deg"))
                .is_some()
        );
    }

    #[test]
    fn losing_a_line_drops_its_angle_only() {
        let mut constraints = with(&[
            "ang hub:1 hub:2 = 30 deg",
            "ang hub:2 hub:3 = 30 deg",
            "dz hub:1 hub:2 = 5 mm",
        ]);
        assert_eq!(constraints.line_lost(Wall::Hub, 1), 1);
        assert_eq!(
            constraints.to_texts(),
            ["ang hub:2 hub:3 = 30 deg", "dz hub:1 hub:2 = 5 mm"]
        );
    }

    #[test]
    fn an_angle_is_stored_between_minus_180_and_180() {
        assert!(close(stored(ConstraintKind::Ang, 270.0), -90.0));
        assert!(close(stored(ConstraintKind::Ang, -180.0), 180.0));
        assert!(close(stored(ConstraintKind::Dz, 15.0), 0.015));
    }

    #[test]
    fn a_dz_puts_the_child_at_parent_plus_d() {
        let mut walls = walls(three_lines());
        let r_before = at(&walls, Wall::Hub, 2)[1];
        let problems = apply(&with(&["dz hub:1 hub:2 = 40 mm"]), [&POINTS; 2], &mut walls);
        assert!(problems.is_empty());
        let (parent, child) = (at(&walls, Wall::Hub, 1), at(&walls, Wall::Hub, 2));
        assert!(close(child[0], parent[0] + 40.0));
        assert!(close(child[1], r_before));
    }

    #[test]
    fn a_dd_puts_the_child_at_parent_diameter_plus_d() {
        let mut walls = walls(three_lines());
        apply(
            &with(&["dd hub:1 hub:2 = -30 mm"]),
            [&POINTS; 2],
            &mut walls,
        );
        let (parent, child) = (at(&walls, Wall::Hub, 1), at(&walls, Wall::Hub, 2));
        assert!(close(2.0 * child[1], 2.0 * parent[1] - 30.0));
    }

    #[test]
    fn enforce_after_the_curve_moves() {
        let mut walls = walls(curve_then_lines());
        let constraints = with(&["dz hub:1 hub:2 = 5 mm"]);
        apply(&constraints, [&POINTS; 2], &mut walls);
        let mut moved = POINTS;
        for point in &mut moved {
            point[0] += 7.0;
        }
        apply(&constraints, [&moved; 2], &mut walls);
        let hub = &walls[wall_index(Wall::Hub)];
        assert!(close(
            hub.node_at(&moved, 2)[0],
            hub.node_at(&moved, 1)[0] + 5.0
        ));
    }

    #[test]
    fn constraints_cross_walls() {
        let mut walls = walls(three_lines());
        walls[wall_index(Wall::Shroud)] = curve_then_lines();
        apply(
            &with(&["dz shroud:1 hub:2 = 10 mm"]),
            [&POINTS; 2],
            &mut walls,
        );
        assert!(close(
            at(&walls, Wall::Hub, 2)[0],
            at(&walls, Wall::Shroud, 1)[0] + 10.0
        ));
    }

    #[test]
    fn chains_apply_parents_first() {
        let mut walls = walls(three_lines());
        let constraints = with(&["dz hub:2 hub:3 = 7 mm", "dz hub:1 hub:2 = 4 mm"]);
        assert!(apply(&constraints, [&POINTS; 2], &mut walls).is_empty());
        assert!(close(
            at(&walls, Wall::Hub, 3)[0],
            at(&walls, Wall::Hub, 1)[0] + 11.0
        ));
    }

    #[test]
    fn a_child_on_the_curve_is_a_problem() {
        let mut walls = walls(curve_then_lines());
        let before = walls.clone();
        let problems = apply(&with(&["dz hub:2 hub:1 = 4 mm"]), [&POINTS; 2], &mut walls);
        assert_eq!(
            problems,
            vec![Problem {
                index: 0,
                reason: "el hijo está sobre la curva".into()
            }]
        );
        assert_eq!(walls, before);
    }

    #[test]
    fn negative_diameter_keeps_the_child() {
        let mut walls = walls(three_lines());
        let before = at(&walls, Wall::Hub, 2);
        let diameter = 2.0 * at(&walls, Wall::Hub, 1)[1];
        let text = format!("dd hub:1 hub:2 = {} mm", -(diameter + 100.0));
        let problems = apply(&with(&[text.as_str()]), [&POINTS; 2], &mut walls);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].reason, "Ø −100 mm no cabe");
        assert_eq!(at(&walls, Wall::Hub, 2), before);
    }

    fn texts(constraints: &Constraints) -> Vec<String> {
        constraints.to_texts()
    }

    fn parsed(text: &str) -> Constraint {
        Constraint::parse(text).unwrap()
    }

    #[test]
    fn cutting_renumbers_the_constraints() {
        let mut constraints = with(&["dz hub:1 hub:2 = 4 mm", "dd shroud:1 shroud:2 = 3 mm"]);
        constraints.node_inserted(Wall::Hub, 1);
        assert_eq!(
            texts(&constraints),
            ["dz hub:2 hub:3 = 4 mm", "dd shroud:1 shroud:2 = 3 mm"]
        );
        constraints.node_inserted(Wall::Hub, 3);
        assert_eq!(texts(&constraints)[0], "dz hub:2 hub:4 = 4 mm");
    }

    #[test]
    fn removing_a_node_drops_its_constraints() {
        let mut constraints = with(&["dz hub:1 hub:2 = 4 mm", "dd hub:3 hub:4 = 3 mm"]);
        assert_eq!(constraints.node_removed(Wall::Hub, 2), 1);
        assert_eq!(texts(&constraints), ["dd hub:2 hub:3 = 3 mm"]);
    }

    #[test]
    fn refusals() {
        let walls = walls(curve_then_lines());
        let constraints = with(&["dz hub:2 hub:3 = 4 mm"]);
        let refusal = |text: &str| constraints.refusal(&walls, &parsed(text));
        assert_eq!(
            refusal("dz hub:2 hub:1 = 1 mm"),
            Some("el hijo tiene que ser un punto libre")
        );
        assert_eq!(
            refusal("dz hub:1 hub:3 = 1 mm"),
            Some("esa cota ya la fija otra")
        );
        assert_eq!(
            refusal("dd hub:2 hub:2 = 1 mm"),
            Some("padre e hijo son el mismo punto")
        );
        assert_eq!(refusal("dd hub:3 hub:2 = 1 mm"), Some("haría un ciclo"));
        assert!(constraints.has_child(Wall::Hub, 3));
        assert!(!constraints.has_child(Wall::Hub, 2));
    }

    #[test]
    fn dz_and_dd_on_the_same_child_are_fine() {
        let walls = walls(curve_then_lines());
        let mut constraints = with(&["dz hub:1 hub:3 = 4 mm"]);
        let new = parsed("dd hub:1 hub:3 = -2 mm");
        assert_eq!(constraints.refusal(&walls, &new), None);
        constraints.add(new);
        constraints.set_value(0, 0.006);
        assert_eq!(
            texts(&constraints),
            ["dz hub:1 hub:3 = 6 mm", "dd hub:1 hub:3 = -2 mm"]
        );
        constraints.remove(0);
        assert_eq!(texts(&constraints), ["dd hub:1 hub:3 = -2 mm"]);
    }

    #[test]
    fn measured_values_of_a_pending_pair() {
        let mut walls = walls(three_lines());
        let hub = &mut walls[wall_index(Wall::Hub)];
        assert!(hub.set_free_axis(&POINTS, 2, 0, 60.0) && hub.set_free_axis(&POINTS, 2, 1, 40.0));
        assert!(hub.set_free_axis(&POINTS, 1, 0, 20.0) && hub.set_free_axis(&POINTS, 1, 1, 50.0));
        let node = |index| NodeRef {
            wall: WallId::Hub,
            index,
        };
        let pending = Pending {
            child: node(2),
            parent: node(1),
        };
        assert_eq!(measured(&walls, [&POINTS; 2], &pending), (40.0, -20.0));
    }

    #[test]
    fn refusal_ends_on_a_cycle_already_in_the_file() {
        let walls = walls(three_lines());
        let constraints = with(&["dz hub:1 hub:2 = 1 mm", "dz hub:2 hub:1 = 1 mm"]);
        assert_eq!(
            constraints.refusal(&walls, &parsed("dd hub:2 hub:3 = 1 mm")),
            None
        );
    }

    /// Recta, arco tangente a ella en el nodo 1, recta: el nodo 2 va sobre
    /// el círculo, que sale de los nodos 0 y 1.
    fn tangent_arc() -> [WallSegments; 2] {
        let mut hub = three_lines();
        assert!(hub.set_piece(1, crate::meridian_segments::Piece::Arc(100.0)));
        assert!(hub.set_tangent(&POINTS, 1, Some(crate::meridian_segments::Tangency::Start)));
        walls(hub)
    }

    #[test]
    fn a_constraint_through_a_tangent_circle_is_a_cycle() {
        let mut walls = tangent_arc();
        let constraints = with(&["dz hub:2 hub:1 = 1 mm"]);
        assert_eq!(
            Constraints::default().refusal(&walls, &parsed("dz hub:2 hub:1 = 1 mm")),
            Some("haría un ciclo")
        );
        let problems = apply(&constraints, [&POINTS; 2], &mut walls);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].reason, "forma un ciclo");
    }

    #[test]
    fn enforce_leaves_an_unreadable_wall_alone() {
        let mut segments = Segments::default();
        segments.walls[wall_index(Wall::Hub)] = three_lines();
        let mut boxes = Meridian::default();
        write(&mut boxes, &segments);
        boxes.segments.constraints = vec!["dz hub:1 hub:2 = 40 mm".into()];
        // Cortes que no crecen: la tapa no se entiende.
        boxes.segments.shroud.cuts = vec!["0.6".into(), "0.3".into()];
        let before = boxes.clone();
        assert!(!enforce_with([&POINTS; 2], &mut boxes));
        assert_eq!(boxes, before);
    }

    fn new_dz(value: f64) -> (Value, Option<Pending>) {
        let node = |index| NodeRef {
            wall: WallId::Hub,
            index,
        };
        let pending = Pending {
            child: node(2),
            parent: node(1),
        };
        let _ = value;
        (Value::New(ConstraintKind::Dz), Some(pending))
    }

    #[test]
    fn typing_that_is_not_a_number_is_refused() {
        let mut segments = Segments::default();
        segments.walls[wall_index(Wall::Hub)] = three_lines();
        let (value, pending) = new_dz(0.0);
        assert_eq!(
            commit(&mut segments, value, pending, "abc"),
            Err("no es un número")
        );
        assert!(segments.constraints.to_texts().is_empty());
        assert_eq!(commit(&mut segments, value, pending, "12,5"), Ok(true));
        assert_eq!(
            segments.constraints.to_texts(),
            ["dz hub:1 hub:2 = 12.5 mm"]
        );
    }

    #[test]
    fn writing_the_same_value_changes_nothing() {
        let mut segments = Segments {
            constraints: with(&["dz hub:1 hub:2 = 40 mm"]),
            ..Segments::default()
        };
        let before = segments.constraints.clone();
        assert_eq!(
            commit(&mut segments, Value::Existing(0), None, "40"),
            Ok(false)
        );
        assert_eq!(segments.constraints, before);
        assert_eq!(
            commit(&mut segments, Value::Existing(0), None, "41"),
            Ok(true)
        );
        assert_eq!(segments.constraints.to_texts(), ["dz hub:1 hub:2 = 41 mm"]);
    }

    #[test]
    fn cutting_forgets_the_unfinished_work() {
        let ctx = egui::Context::default();
        let (value, pending) = new_dz(0.0);
        ctx.data_mut(|data| {
            data.insert_temp(pending_id(), pending);
            data.insert_temp(
                writing_id(),
                Some(Writing {
                    value,
                    pending,
                    text: String::new(),
                    screen: egui::Pos2::ZERO,
                    fresh: false,
                    refusal: None,
                }),
            );
        });
        forget_unfinished(&ctx);
        assert_eq!(
            ctx.data(|data| data.get_temp::<Option<Pending>>(pending_id())),
            Some(None)
        );
        assert!(
            ctx.data(|data| data.get_temp::<Option<Writing>>(writing_id()))
                .flatten()
                .is_none()
        );
    }

    #[test]
    fn enforce_writes_once() {
        let mut segments = Segments::default();
        segments.walls[wall_index(Wall::Hub)] = three_lines();
        let mut boxes = Meridian::default();
        write(&mut boxes, &segments);
        boxes.segments.constraints = vec!["dz hub:1 hub:2 = 40 mm".into()];
        assert!(enforce_with([&POINTS; 2], &mut boxes));
        assert!(!enforce_with([&POINTS; 2], &mut boxes));
        assert_eq!(boxes.segments.constraints, ["dz hub:1 hub:2 = 40 mm"]);
    }

    #[test]
    fn unreadable_texts_survive() {
        let texts = [
            "dz hub:1",
            "ang hub:1 hub:2 = 30 deg",
            "dz hub:1 hub:2 = 5 mm",
        ];
        assert_eq!(with(&texts).to_texts(), texts);
    }
}
