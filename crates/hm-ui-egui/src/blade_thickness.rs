//! Gráfica «Espesor» del álabe 3D: la curva e(x) de cubo y tapa con nodos que
//! se arrastran.
//!
//! La abscisa es la de β(x): x = 0 en el borde de ataque y x = 1 en la salida.
//! Las cotas van en milímetros y con coma decimal; `hm-design` trabaja en
//! metros. El arrastre sigue el de las manijas de β (`blade_beta`): se agarra el
//! nodo a menos de `GRAB_PIXELS` y el puntero da (x, e).

use eframe::egui;
use egui_plot::{Plot, PlotPoint, Points, Text};
use hm_design::blade_camber::BladeCamber;
use hm_design::blade_thickness::{
    BladeThickness, THICKNESS_WARNING, ThicknessCurves, ThicknessWall, node_floor,
    thickness_curves_for_design,
};
use hm_design::mechanics::CAST_MIN_THICKNESS;

use crate::blade_beta::{BetaHistory, BetaSnapshot, GRAB_PIXELS};
use crate::curves::line;
use crate::model::{Inputs, Outcome};

/// Lo que dibuja la gráfica: las curvas que se ven y el estado guardado.
pub(super) struct ThicknessGraph {
    /// Las curvas de cubo y tapa tal como las ve el diseño, en m.
    pub curves: ThicknessCurves,
    /// «Cubo = tapa» y los nodos guardados, si los hay.
    pub setup: BladeThickness,
}

impl ThicknessGraph {
    /// La gráfica de este diseño; el error de unos nodos guardados que no
    /// valen, con su texto.
    pub fn of(outcome: &Outcome, camber: &BladeCamber) -> Result<Self, String> {
        let setup = outcome.inputs.blade.thickness.clone();
        let curves = thickness_curves_for_design(&outcome.result, camber, &setup)
            .map_err(|error| error.to_string())?;
        Ok(Self { curves, setup })
    }

    /// Las paredes que se dibujan: una sola con «cubo = tapa».
    fn walls(&self) -> Vec<ThicknessWall> {
        if self.setup.same {
            vec![ThicknessWall::Hub]
        } else {
            vec![ThicknessWall::Hub, ThicknessWall::Shroud]
        }
    }

    /// Las curvas muestreadas, [x, e en mm]; una sola con «cubo = tapa».
    pub fn lines(&self, samples: usize) -> Vec<(ThicknessWall, Vec<[f64; 2]>)> {
        let samples = samples.max(2);
        self.walls()
            .into_iter()
            .map(|wall| {
                let curve = match wall {
                    ThicknessWall::Hub => &self.curves.hub,
                    ThicknessWall::Shroud => &self.curves.shroud,
                };
                let points = (0..samples)
                    .map(|i| {
                        let x = i as f64 / (samples - 1) as f64;
                        [x, curve.at(x) * 1000.0]
                    })
                    .collect();
                (wall, points)
            })
            .collect()
    }

    /// Los nodos de una pared, [x, e en mm].
    pub fn nodes(&self, wall: ThicknessWall) -> Vec<[f64; 2]> {
        let curve = match wall {
            ThicknessWall::Hub => &self.curves.hub,
            ThicknessWall::Shroud => &self.curves.shroud,
        };
        curve.nodes().iter().map(|n| [n.x, n.e * 1000.0]).collect()
    }

    /// Un texto por nodo entre su mínimo y los 5 mm: Gülich, §7.2.1, da «3–5
    /// mm» como mínimo de fundición.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for wall in self.walls() {
            for (index, [x, e]) in self.nodes(wall).into_iter().enumerate() {
                if e < THICKNESS_WARNING * 1000.0 {
                    out.push(format!(
                        "{} · nodo {} (x = {}): {} mm, entre el mínimo y 5 mm",
                        self.wall_name(wall),
                        index + 1,
                        comma(x, 2),
                        mm_text(e),
                    ));
                }
            }
        }
        out
    }

    /// «Cubo», «Tapa» o «Cubo y tapa» con el interruptor encendido.
    fn wall_name(&self, wall: ThicknessWall) -> &'static str {
        match (self.setup.same, wall) {
            (true, _) => "Cubo y tapa",
            (false, ThicknessWall::Hub) => "Cubo",
            (false, ThicknessWall::Shroud) => "Tapa",
        }
    }

    /// Todos los nodos que se pueden agarrar: (pared, índice, [x, e mm]).
    fn handles(&self) -> Vec<(ThicknessWall, usize, [f64; 2])> {
        self.walls()
            .into_iter()
            .flat_map(|wall| {
                self.nodes(wall)
                    .into_iter()
                    .enumerate()
                    .map(move |(index, point)| (wall, index, point))
            })
            .collect()
    }
}

/// «Gülich» si la curva sigue al diseño, «editada» si hay nodos guardados.
pub(super) fn state_label(setup: &BladeThickness) -> &'static str {
    if setup.is_edited() {
        "editada"
    } else {
        "Gülich"
    }
}

/// Un número con coma decimal.
fn comma(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

/// Una cota en mm, con dos decimales y coma: «6,00».
fn mm_text(value: f64) -> String {
    comma(value, 2)
}

/// La gráfica, el interruptor y el estado. Devuelve `true` si se escribió
/// `inputs` y hay que recalcular.
pub(super) fn thickness_view(
    ui: &mut egui::Ui,
    outcome: &Outcome,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
    camber: &BladeCamber,
) -> bool {
    let graph = match ThicknessGraph::of(outcome, camber) {
        Ok(graph) => graph,
        Err(message) => {
            ui.label(format!("Sin curva de espesor: {message}."));
            return false;
        }
    };
    let mut edited = false;
    let time = ui.input(|input| input.time);
    ui.horizontal(|ui| {
        let mut same = graph.setup.same;
        if ui
            .checkbox(&mut same, "cubo = tapa")
            .on_hover_text(
                "Encendido, una sola curva vale para cubo y tapa. Apagado, la tapa arranca \
                 como copia del cubo.",
            )
            .changed()
        {
            inputs.blade.thickness = graph.setup.with_same(&graph.curves, same);
            history.record(&BetaSnapshot::of(inputs), time);
            edited = true;
        }
        ui.add_space(8.0);
        ui.label(format!("Espesor: {}", state_label(&graph.setup)));
        let back = ui
            .add_enabled(
                graph.setup.is_edited(),
                egui::Button::new("Volver a Gülich"),
            )
            .on_hover_text("Borra los nodos guardados: la curva vuelve a seguir al diseño.");
        if back.clicked() {
            inputs.blade.thickness = graph.setup.reset();
            history.record(&BetaSnapshot::of(inputs), time);
            edited = true;
        }
    });
    ui.label(
        egui::RichText::new(
            "e(x) en mm; x es la fracción de la longitud meridional del álabe. Arrastra los \
             nodos: el espesor no baja del mínimo de fundición, 3 mm (1,5 mm en la salida).",
        )
        .small()
        .weak(),
    );
    edited |= thickness_plot(ui, &graph, inputs, history);
    if !graph.warnings().is_empty() {
        ui.horizontal_wrapped(|ui| {
            for warning in graph.warnings() {
                ui.label(egui::RichText::new(warning).small().weak());
            }
        });
    }
    edited
}

/// El trazado con sus nodos y el arrastre. `true` si escribió `inputs`.
fn thickness_plot(
    ui: &mut egui::Ui,
    graph: &ThicknessGraph,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
) -> bool {
    let status_id = egui::Id::unique("blade_thickness_status");
    let palette = crate::theme::palette(ui.ctx());
    let mut written = false;
    // Margen arriba para que la cota del nodo más alto no quede cortada.
    let top = graph
        .handles()
        .iter()
        .map(|(_, _, point)| point[1])
        .fold(CAST_MIN_THICKNESS * 1000.0, f64::max);
    Plot::new("blade_thickness")
        .height(190.0)
        .include_y(0.0)
        .include_y(top * 1.25)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .x_axis_label("x, fracción de longitud meridional")
        .y_axis_label("e, mm")
        .show(ui, |plot_ui| {
            let minimum = CAST_MIN_THICKNESS * 1000.0;
            plot_ui.line(
                line(
                    "mínimo de fundición",
                    vec![[0.0, minimum], [1.0, minimum]],
                    palette.error,
                )
                .width(1.0)
                .style(egui_plot::LineStyle::dashed_loose()),
            );
            plot_ui.text(
                Text::new(
                    "",
                    PlotPoint::new(0.0, minimum),
                    egui::RichText::new(" mínimo de fundición")
                        .small()
                        .color(palette.error),
                )
                .anchor(egui::Align2::LEFT_BOTTOM),
            );
            for (wall, points) in graph.lines(120) {
                let color = wall_color(palette, graph.setup.same, wall);
                plot_ui.line(line(graph.wall_name(wall), points, color).width(2.5));
            }
            for wall in graph.walls() {
                let color = wall_color(palette, graph.setup.same, wall);
                let nodes = graph.nodes(wall);
                plot_ui.points(
                    Points::new(graph.wall_name(wall), nodes.clone())
                        .color(color)
                        .radius(6.0),
                );
                for [x, e] in nodes {
                    plot_ui.text(
                        Text::new(
                            "",
                            PlotPoint::new(x, e),
                            egui::RichText::new(format!(" {}", mm_text(e))).small(),
                        )
                        .anchor(if x > 0.9 {
                            egui::Align2::RIGHT_TOP
                        } else {
                            egui::Align2::LEFT_BOTTOM
                        }),
                    );
                }
            }
            if let Some(drag) = drag_nodes(plot_ui, graph, inputs) {
                match drag {
                    NodeDrag::Moving { status, written: w } => {
                        plot_ui
                            .ctx()
                            .data_mut(|data| data.insert_temp(status_id, status));
                        written |= w;
                    }
                    NodeDrag::Stopped => {
                        plot_ui
                            .ctx()
                            .data_mut(|data| data.remove::<String>(status_id));
                    }
                }
            }
        });
    if written {
        history.record(&BetaSnapshot::of(inputs), ui.input(|input| input.time));
    }
    if let Some(status) = ui.ctx().data(|data| data.get_temp::<String>(status_id)) {
        ui.label(egui::RichText::new(status).small());
    }
    written
}

/// Color de la curva de una pared: la del álabe con «cubo = tapa».
fn wall_color(palette: &crate::theme::Palette, same: bool, wall: ThicknessWall) -> egui::Color32 {
    match (same, wall) {
        (true, _) => palette.blade,
        (false, ThicknessWall::Hub) => palette.hub,
        (false, ThicknessWall::Shroud) => palette.shroud,
    }
}

/// Lo que pasó en un fotograma de arrastre.
enum NodeDrag {
    /// Se mueve un nodo: la línea de lectura y si se escribió `inputs`.
    Moving { status: String, written: bool },
    /// Se soltó: la lectura se quita.
    Stopped,
}

/// Arrastra los nodos: se agarra el más cercano a menos de [`GRAB_PIXELS`]; el
/// puntero da (x, e en mm) y [`BladeThickness::with_node_moved`] lo acota.
fn drag_nodes(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    graph: &ThicknessGraph,
    inputs: &mut Inputs,
) -> Option<NodeDrag> {
    let id = egui::Id::unique("blade_thickness_dragged");
    let response = plot_ui.response().clone();
    if response.drag_stopped() {
        plot_ui
            .ctx()
            .data_mut(|data| data.remove::<Option<(ThicknessWall, usize)>>(id));
        return Some(NodeDrag::Stopped);
    }
    let pointer = plot_ui.pointer_coordinate()?;
    if response.drag_started() {
        let at = plot_ui.screen_from_plot(pointer);
        let grabbed = graph
            .handles()
            .into_iter()
            .map(|(wall, index, point)| {
                let screen = plot_ui.screen_from_plot(PlotPoint::new(point[0], point[1]));
                ((wall, index), screen.distance(at))
            })
            .filter(|(_, distance)| *distance <= GRAB_PIXELS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(handle, _)| handle);
        plot_ui.ctx().data_mut(|data| data.insert_temp(id, grabbed));
    }
    let grabbed: Option<(ThicknessWall, usize)> =
        plot_ui.ctx().data(|data| data.get_temp(id)).flatten();
    let (Some((wall, index)), true) = (grabbed, response.dragged()) else {
        return None;
    };
    let moved =
        graph
            .setup
            .with_node_moved(&graph.curves, wall, index, pointer.x, pointer.y / 1000.0);
    let written = moved != inputs.blade.thickness;
    inputs.blade.thickness = moved;
    // Lo que quedó: el nodo ya acotado por la curva nueva.
    let shown = inputs
        .blade
        .thickness
        .edited
        .as_ref()
        .and_then(|edited| {
            let nodes = match wall {
                ThicknessWall::Hub => &edited.hub,
                ThicknessWall::Shroud => &edited.shroud,
            };
            nodes.get(index).copied()
        })
        .map_or([pointer.x, pointer.y], |node| [node.x, node.e * 1000.0]);
    let floor = node_floor(shown[0]) * 1000.0;
    let mut status = format!(
        "{} · nodo {}: x = {} · e = {} mm",
        graph.wall_name(wall),
        index + 1,
        comma(shown[0], 2),
        mm_text(shown[1]),
    );
    if shown[1] <= floor + 1e-9 {
        status.push_str(&format!(" (mínimo de fundición: {} mm)", comma(floor, 1)));
    }
    Some(NodeDrag::Moving { status, written })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blade_beta::{BetaHistory, BetaSnapshot};
    use crate::model::{Basis, Inputs, Outcome};
    use hm_design::blade_camber::BladeCamber;
    use hm_design::blade_setup::BladeShape;
    use hm_design::blade_thickness::{BladeThickness, ThicknessWall};

    /// B1 con forma libre 3D y cinco líneas de corriente: el resultado y su
    /// línea media.
    fn b1_3d_with(thickness: BladeThickness) -> (Outcome, BladeCamber) {
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free3D;
        inputs.blade.streamlines = 5;
        inputs.blade.thickness = thickness;
        let outcome = crate::model::compute(&inputs, &Basis::default()).unwrap();
        let camber = crate::blade_inlet::camber_of(&outcome)
            .unwrap()
            .expect("forma 3D");
        (outcome, camber)
    }

    fn b1_3d() -> (Outcome, BladeCamber) {
        b1_3d_with(BladeThickness::default())
    }

    fn graph_of(thickness: BladeThickness) -> ThicknessGraph {
        let (outcome, camber) = b1_3d_with(thickness);
        ThicknessGraph::of(&outcome, &camber).unwrap()
    }

    #[test]
    fn the_graph_starts_in_gulich_with_one_curve() {
        let graph = graph_of(BladeThickness::default());
        assert_eq!(graph.lines(50).len(), 1);
        assert_eq!(state_label(&graph.setup), "Gülich");
        assert_eq!(graph.nodes(ThicknessWall::Hub).len(), 3);
    }

    #[test]
    fn the_nodes_are_in_millimetres() {
        let (outcome, camber) = b1_3d();
        let graph = ThicknessGraph::of(&outcome, &camber).unwrap();
        let r = &outcome.result;
        let (_, e, e_trailing) = hm_design::mechanics::blade_thicknesses(
            r.selection.blade_count,
            r.u2.si,
            r.nq,
            r.d2.si,
        )
        .unwrap();
        let nodes = graph.nodes(ThicknessWall::Hub);
        assert!((nodes[0][1] - 1000.0 * e).abs() < 1e-9);
        assert!((nodes[2][1] - 1000.0 * e_trailing).abs() < 1e-9);
        assert_eq!(nodes[0][0], 0.0);
        assert_eq!(nodes[2][0], 1.0);
    }

    #[test]
    fn dragging_a_node_makes_the_curve_edited() {
        let graph = graph_of(BladeThickness::default());
        let moved = graph
            .setup
            .with_node_moved(&graph.curves, ThicknessWall::Hub, 0, 0.0, 0.006);
        let after = graph_of(moved);
        assert_eq!(state_label(&after.setup), "editada");
        assert!((after.nodes(ThicknessWall::Hub)[0][1] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn back_to_gulich_restores_the_design_curve() {
        let start = graph_of(BladeThickness::default());
        let moved = start
            .setup
            .with_node_moved(&start.curves, ThicknessWall::Hub, 0, 0.0, 0.006);
        let edited = graph_of(moved);
        let back = graph_of(edited.setup.reset());
        assert_eq!(state_label(&back.setup), "Gülich");
        assert_eq!(
            back.nodes(ThicknessWall::Hub),
            start.nodes(ThicknessWall::Hub)
        );
    }

    #[test]
    fn the_switch_splits_and_joins_hub_and_shroud() {
        let start = graph_of(BladeThickness::default());
        let split = graph_of(start.setup.with_same(&start.curves, false));
        let lines = split.lines(50);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].1, lines[1].1);
        // Mover la tapa no toca el cubo.
        let moved =
            split
                .setup
                .with_node_moved(&split.curves, ThicknessWall::Shroud, 1, 0.6, 0.008);
        let moved = graph_of(moved);
        let lines = moved.lines(50);
        assert_eq!(lines.len(), 2);
        assert_ne!(lines[0].1, lines[1].1);
        assert_eq!(
            moved.nodes(ThicknessWall::Hub),
            split.nodes(ThicknessWall::Hub)
        );
        // Encenderlo: una sola curva, la del cubo.
        let joined = graph_of(moved.setup.with_same(&moved.curves, true));
        let lines = joined.lines(50);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].0, ThicknessWall::Hub);
        assert_eq!(lines[0].1, moved.lines(50)[0].1);
    }

    #[test]
    fn undo_restores_the_thickness_nodes() {
        let (outcome, _) = b1_3d();
        let graph = graph_of(BladeThickness::default());
        let before = BetaSnapshot::of(&outcome.inputs);
        let mut history = BetaHistory::default();
        history.follow(&before);
        let mut inputs = outcome.inputs.clone();
        inputs.blade.thickness =
            graph
                .setup
                .with_node_moved(&graph.curves, ThicknessWall::Hub, 0, 0.0, 0.006);
        assert!(inputs.blade.thickness.is_edited());
        history.record(&BetaSnapshot::of(&inputs), 1.0);
        let previous = history
            .undo(&BetaSnapshot::of(&inputs))
            .expect("hay un paso que deshacer");
        previous.apply(&mut inputs);
        assert_eq!(inputs.blade.thickness, BladeThickness::default());
    }

    #[test]
    fn a_2d_shape_keeps_and_saves_the_nodes() {
        let graph = graph_of(BladeThickness::default());
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free2D;
        inputs.blade.thickness =
            graph
                .setup
                .with_node_moved(&graph.curves, ThicknessWall::Hub, 0, 0.0, 0.006);
        let built = crate::model::build_project(&inputs, &Basis::default(), None, "B1").unwrap();
        let blade = built.project.blade.expect("[blade] con los nodos");
        assert!(!blade.thickness_hub.is_empty());
        assert!((blade.thickness_hub[0][1] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn thin_nodes_raise_a_warning() {
        let graph = graph_of(BladeThickness::default());
        assert!(!graph.warnings().is_empty());
    }

    #[test]
    fn the_view_draws_without_panicking() {
        for edited in [false, true] {
            let graph = graph_of(BladeThickness::default());
            let thickness = if edited {
                graph
                    .setup
                    .with_node_moved(&graph.curves, ThicknessWall::Hub, 0, 0.0, 0.006)
            } else {
                BladeThickness::default()
            };
            let (outcome, camber) = b1_3d_with(thickness);
            let mut inputs = outcome.inputs.clone();
            let mut history = BetaHistory::default();
            egui::__run_test_ui(|ui| {
                thickness_view(ui, &outcome, &mut inputs, &mut history, &camber);
            });
        }
    }

    #[test]
    fn the_millimetres_have_a_decimal_comma() {
        assert_eq!(mm_text(3.2105), "3,21");
        assert_eq!(mm_text(6.0), "6,00");
    }
}
