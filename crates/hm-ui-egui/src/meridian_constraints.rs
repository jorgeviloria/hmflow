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
use crate::meridian_segments::{Segments, WallSegments, read, trimmed, wall_index, write};

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

    /// Las cotas Δz y ΔØ, con su índice en la lista.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &Constraint)> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                Item::Read(constraint, _) if constraint.kind != ConstraintKind::Ang => {
                    Some((index, constraint))
                }
                _ => None,
            })
    }
}

pub(super) fn wall_of(id: WallId) -> Wall {
    match id {
        WallId::Shroud => Wall::Shroud,
        WallId::Hub => Wall::Hub,
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

/// Dibuja las cotas que mandan: una línea fina entre la coordenada del padre
/// y la del hijo y su valor; en rojo y con el motivo las que no se cumplen.
/// Devuelve, por cada valor, su rectángulo en pantalla y su índice.
pub(super) fn draw(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &Segments,
) -> Vec<(egui::Rect, usize)> {
    let points = wall_points(model);
    let mut walls = segments.walls.clone();
    let problems = apply(&segments.constraints, points, &mut walls);
    let mut rects = Vec::new();
    for (index, constraint) in segments.constraints.iter() {
        let place = |node: NodeRef| (wall_index(wall_of(node.wall)), node.index);
        let ((pw, p), (cw, c)) = (place(constraint.parent), place(constraint.child));
        if p >= walls[pw].node_count() || c >= walls[cw].node_count() {
            continue;
        }
        let (from, to) = (walls[pw].node_at(points[pw], p), walls[cw].node_at(points[cw], c));
        let value = trimmed(constraint.value * 1000.0, 2).replace('-', "−");
        let (ends, label) = match constraint.kind {
            ConstraintKind::Dz => ([[from[0], to[1]], to], format!("Δz {value}")),
            ConstraintKind::Dd => ([[to[0], from[1]], to], format!("ΔØ {value}")),
            ConstraintKind::Ang => continue,
        };
        let problem = problems.iter().find(|problem| problem.index == index);
        let (colour, text) = match problem {
            Some(problem) => (BROKEN, format!("{label} · {}", problem.reason)),
            None => (COMMANDING, label),
        };
        plot_ui.line(line("Cota que manda", ends.to_vec(), colour).width(1.0));
        let middle = [(ends[0][0] + ends[1][0]) / 2.0, (ends[0][1] + ends[1][1]) / 2.0];
        plot_ui.text(
            egui_plot::Text::new(
                "Cota que manda",
                PlotPoint::new(middle[0], middle[1]),
                egui::RichText::new(text).small().color(colour),
            )
            .anchor(egui::Align2::CENTER_BOTTOM),
        );
        let screen = plot_ui.screen_from_plot(PlotPoint::new(middle[0], middle[1]));
        rects.push((
            egui::Rect::from_center_size(screen - egui::vec2(0.0, 7.0), egui::vec2(60.0, 14.0)),
            index,
        ));
    }
    rects
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
    boxes.segments.constraints = text
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
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
            .filter(|(_, c)| !pending.iter().any(|(_, other)| other.child == c.parent))
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
            if let Err(reason) = apply_one(&constraint, points, walls) {
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
        ConstraintKind::Ang => return Ok(()),
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
        apply(&with(&["dd hub:1 hub:2 = -30 mm"]), [&POINTS; 2], &mut walls);
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
        assert!(close(hub.node_at(&moved, 2)[0], hub.node_at(&moved, 1)[0] + 5.0));
    }

    #[test]
    fn constraints_cross_walls() {
        let mut walls = walls(three_lines());
        walls[wall_index(Wall::Shroud)] = curve_then_lines();
        apply(&with(&["dz shroud:1 hub:2 = 10 mm"]), [&POINTS; 2], &mut walls);
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
