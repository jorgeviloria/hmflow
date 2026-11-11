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
    BladeThickness, THICKNESS_NODES, THICKNESS_WARNING, ThicknessCurves, ThicknessWall, node_floor,
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
    if graph.setup.is_edited()
        && let Some((inlet, exit)) = outcome.result.blade_edge_thickness
    {
        ui.label(
            egui::RichText::new(format!(
                "El 1D usa e(0) = {} mm y e(1) = {} mm (línea central) para el bloqueo.",
                mm_text(inlet * 1000.0),
                mm_text(exit * 1000.0)
            ))
            .small()
            .weak(),
        );
    }
    if !graph.warnings().is_empty() {
        ui.horizontal_wrapped(|ui| {
            for warning in graph.warnings() {
                ui.label(egui::RichText::new(warning).small().weak());
            }
        });
    }
    edited
}

/// El trazado con sus nodos, el arrastre, el menú de clic derecho y la caja de
/// la cota. `true` si escribió `inputs`.
fn thickness_plot(
    ui: &mut egui::Ui,
    graph: &ThicknessGraph,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
) -> bool {
    let status_id = egui::Id::unique("blade_thickness_status");
    let target_id = egui::Id::unique("blade_thickness_target");
    let editing_id = egui::Id::unique("blade_thickness_editing");
    let palette = crate::theme::palette(ui.ctx());
    let time = ui.input(|input| input.time);
    let mut written = false;
    // Margen arriba para que la cota del nodo más alto no quede cortada.
    let top = graph
        .handles()
        .iter()
        .map(|(_, _, point)| point[1])
        .fold(CAST_MIN_THICKNESS * 1000.0, f64::max);
    let plot = Plot::new("blade_thickness")
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
                        .anchor(label_anchor(x)),
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
            // Lo que hay bajo el puntero cuando se hace clic derecho o doble
            // clic: el menú y la caja de la cota lo leen después.
            let response = plot_ui.response().clone();
            if let Some(pointer) = plot_ui.pointer_coordinate() {
                let to_screen = |p: [f64; 2]| plot_ui.screen_from_plot(PlotPoint::new(p[0], p[1]));
                let at = plot_ui.screen_from_plot(pointer);
                if response.secondary_clicked() {
                    let target = target_at(graph, &to_screen, at);
                    plot_ui
                        .ctx()
                        .data_mut(|data| data.insert_temp(target_id, target));
                }
                if response.double_clicked()
                    && let Some((wall, index)) = label_hit(graph, &to_screen, at)
                {
                    let e = graph.nodes(wall).get(index).map_or(0.0, |node| node[1]);
                    plot_ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            editing_id,
                            Editing {
                                wall,
                                index,
                                text: comma(e, 2),
                                opened: true,
                            },
                        );
                    });
                }
            }
        });
    // Clic derecho: «Añadir nodo aquí» sobre la curva, «Quitar nodo» sobre un
    // nodo.
    let target = ui
        .ctx()
        .data(|data| data.get_temp::<Target>(target_id))
        .unwrap_or(Target::Nothing);
    plot.response.context_menu(|ui| match target {
        Target::Node(wall, index) => {
            let can = can_remove(graph, wall, index);
            let remove = ui.add_enabled(can.is_ok(), egui::Button::new("Quitar nodo"));
            if let Err(reason) = can {
                ui.label(egui::RichText::new(reason).small().weak());
            }
            if remove.clicked() {
                written |= remove_node(inputs, history, graph, wall, index, time);
                ui.close();
            }
        }
        Target::Curve(wall, x) => {
            let can = can_add(graph, wall, x);
            let add = ui.add_enabled(can.is_ok(), egui::Button::new("Añadir nodo aquí"));
            if let Err(reason) = can {
                ui.label(egui::RichText::new(reason).small().weak());
            }
            if add.clicked() {
                written |= add_node(inputs, history, graph, wall, x, time);
                ui.close();
            }
        }
        Target::Nothing => {
            ui.label(
                egui::RichText::new("Clic derecho sobre un nodo o sobre la curva.")
                    .small()
                    .weak(),
            );
        }
    });
    // Doble clic en la cota: una caja de texto. Intro la aplica, Esc la cierra.
    if let Some(mut editing) = ui.ctx().data(|data| data.get_temp::<Editing>(editing_id)) {
        let node = graph
            .nodes(editing.wall)
            .get(editing.index)
            .copied()
            .map(|[x, e]| plot.transform.position_from_point(&PlotPoint::new(x, e)));
        let Some(node) = node else {
            // El nodo ya no existe (se quitó o se deshizo): se cierra la caja.
            ui.ctx().data_mut(|data| data.remove::<Editing>(editing_id));
            return written;
        };
        let mut outcome = BoxOutcome::Open;
        egui::Area::new(editing_id.with("area"))
            .order(egui::Order::Foreground)
            .fixed_pos(node + egui::vec2(8.0, -26.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let response = ui
                            .add(egui::TextEdit::singleline(&mut editing.text).desired_width(56.0));
                        ui.label(egui::RichText::new("mm").small());
                        if editing.opened {
                            response.request_focus();
                            editing.opened = false;
                        }
                        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                        let enter = response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if escape {
                            outcome = BoxOutcome::Cancel;
                        } else if enter {
                            outcome = BoxOutcome::Apply;
                        } else if response.lost_focus() {
                            outcome = BoxOutcome::Cancel;
                        }
                    });
                });
            });
        match outcome {
            BoxOutcome::Open => {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(editing_id, editing));
            }
            BoxOutcome::Cancel => {
                ui.ctx().data_mut(|data| data.remove::<Editing>(editing_id));
            }
            BoxOutcome::Apply => {
                ui.ctx().data_mut(|data| data.remove::<Editing>(editing_id));
                let status =
                    match write_thickness(graph, editing.wall, editing.index, &editing.text) {
                        Ok((setup, note)) => {
                            let changed = setup != inputs.blade.thickness;
                            inputs.blade.thickness = setup;
                            if changed {
                                history.record(&BetaSnapshot::of(inputs), time);
                                written = true;
                            }
                            let mut status = format!(
                                "{} · nodo {}: e escrito = {} mm",
                                graph.wall_name(editing.wall),
                                editing.index + 1,
                                editing.text.trim()
                            );
                            if let Some(note) = note {
                                status.push_str(&format!(" ({note})"));
                            }
                            status
                        }
                        Err(reason) => format!("No se cambia el nodo: {reason}"),
                    };
                ui.ctx()
                    .data_mut(|data| data.insert_temp(status_id, status));
            }
        }
    }
    if let Some(status) = ui.ctx().data(|data| data.get_temp::<String>(status_id)) {
        ui.label(egui::RichText::new(status).small());
    }
    written
}

/// Dónde se ancla la cota de un nodo: a la derecha y arriba, salvo cerca de la
/// salida, donde va a la izquierda y debajo para no salirse del marco.
fn label_anchor(x: f64) -> egui::Align2 {
    if x > 0.9 {
        egui::Align2::RIGHT_TOP
    } else {
        egui::Align2::LEFT_BOTTOM
    }
}

/// Lo que hay bajo el puntero en la gráfica.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    /// Un nodo: pared e índice.
    Node(ThicknessWall, usize),
    /// Un punto de la curva que no es un nodo: pared y x.
    Curve(ThicknessWall, f64),
    Nothing,
}

/// El nodo a menos de [`GRAB_PIXELS`] del puntero; si no, el punto de la curva
/// a menos de esa distancia; si no, nada. `to_screen` pasa (x, e mm) a píxeles.
fn target_at(
    graph: &ThicknessGraph,
    to_screen: &dyn Fn([f64; 2]) -> egui::Pos2,
    pointer: egui::Pos2,
) -> Target {
    let node = graph
        .handles()
        .into_iter()
        .map(|(wall, index, point)| ((wall, index), to_screen(point).distance(pointer)))
        .filter(|(_, distance)| *distance <= GRAB_PIXELS)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some(((wall, index), _)) = node {
        return Target::Node(wall, index);
    }
    let mut best: Option<(ThicknessWall, f64, f32)> = None;
    for (wall, points) in graph.lines(120) {
        for pair in points.windows(2) {
            let (a, b) = (to_screen(pair[0]), to_screen(pair[1]));
            let along = b - a;
            let length_squared = along.length_sq();
            let t = if length_squared > 0.0 {
                (((pointer - a).dot(along)) / length_squared).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let distance = (a + along * t).distance(pointer);
            if distance <= GRAB_PIXELS && best.is_none_or(|(_, _, d)| distance < d) {
                let x = pair[0][0] + f64::from(t) * (pair[1][0] - pair[0][0]);
                best = Some((wall, x, distance));
            }
        }
    }
    best.map_or(Target::Nothing, |(wall, x, _)| Target::Curve(wall, x))
}

/// El nodo cuya cota (o cuyo punto) está bajo el puntero: ahí el doble clic
/// abre la caja de texto.
fn label_hit(
    graph: &ThicknessGraph,
    to_screen: &dyn Fn([f64; 2]) -> egui::Pos2,
    pointer: egui::Pos2,
) -> Option<(ThicknessWall, usize)> {
    // Tamaño aproximado de la cota, en píxeles, con letra pequeña.
    const LABEL: egui::Vec2 = egui::vec2(52.0, 16.0);
    graph
        .handles()
        .into_iter()
        .filter(|(_, _, point)| {
            let p = to_screen(*point);
            let rect = match label_anchor(point[0]) {
                a if a == egui::Align2::RIGHT_TOP => {
                    egui::Rect::from_min_size(p + egui::vec2(-LABEL.x, 0.0), LABEL)
                }
                _ => egui::Rect::from_min_size(p + egui::vec2(0.0, -LABEL.y), LABEL),
            };
            rect.contains(pointer) || p.distance(pointer) <= GRAB_PIXELS
        })
        .min_by(|a, b| {
            let d = |h: &(ThicknessWall, usize, [f64; 2])| to_screen(h.2).distance(pointer);
            d(a).total_cmp(&d(b))
        })
        .map(|(wall, index, _)| (wall, index))
}

/// La caja de texto de una cota abierta con doble clic.
#[derive(Debug, Clone)]
struct Editing {
    wall: ThicknessWall,
    index: usize,
    text: String,
    /// Recién abierta: pide el foco una vez.
    opened: bool,
}

/// Qué pasó con la caja en este fotograma.
enum BoxOutcome {
    Open,
    Apply,
    Cancel,
}

/// «4,5», «4.5» o «4,5 mm» → 4.5. Vacío, texto o un valor que no es mayor que
/// cero: `Err` con la frase que se enseña.
pub(super) fn parse_thickness_mm(text: &str) -> Result<f64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("escribe un espesor en mm, por ejemplo 4,5".to_string());
    }
    let number = trimmed.to_lowercase();
    let number = number.strip_suffix("mm").unwrap_or(&number).trim();
    let value: f64 = number
        .replace(',', ".")
        .parse()
        .map_err(|_| format!("«{trimmed}» no es un espesor en mm; escribe por ejemplo 4,5"))?;
    if !value.is_finite() {
        return Err(format!("«{trimmed}» no es un número finito"));
    }
    if value <= 0.0 {
        return Err("el espesor tiene que ser mayor que cero".to_string());
    }
    Ok(value)
}

/// Un número en mm con coma y sin ceros de sobra: «3», «1,5».
fn mm_trimmed(value: f64) -> String {
    crate::meridian_segments::trimmed(value, 2).replace('.', ",")
}

/// Aplica una cota escrita al nodo `index` de `wall`, con su misma x. Da el
/// estado nuevo y, si el valor bajaba del mínimo de fundición y se subió a él,
/// la nota que se enseña. Un texto que no vale o un nodo que no existe es
/// `Err` y no cambia nada.
fn write_thickness(
    graph: &ThicknessGraph,
    wall: ThicknessWall,
    index: usize,
    text: &str,
) -> Result<(BladeThickness, Option<String>), String> {
    let mm = parse_thickness_mm(text)?;
    let Some(&[x, _]) = graph.nodes(wall).get(index) else {
        return Err("ese nodo ya no existe".to_string());
    };
    let floor = node_floor(x);
    let setup = graph
        .setup
        .with_node_moved(&graph.curves, wall, index, x, mm / 1000.0);
    let note = (mm / 1000.0 < floor)
        .then(|| format!("mínimo de fundición: {} mm", mm_trimmed(floor * 1000.0)));
    Ok((setup, note))
}

/// Si se puede añadir un nodo en x sobre la curva de `wall`; si no, por qué.
fn can_add(graph: &ThicknessGraph, wall: ThicknessWall, x: f64) -> Result<(), &'static str> {
    if graph.nodes(wall).len() >= THICKNESS_NODES.1 {
        return Err("máximo 8 nodos");
    }
    match graph.setup.with_node_added(&graph.curves, wall, x) {
        Some(_) => Ok(()),
        None => Err("demasiado cerca de otro nodo"),
    }
}

/// Si se puede quitar el nodo `index`; si no, por qué.
fn can_remove(
    graph: &ThicknessGraph,
    wall: ThicknessWall,
    index: usize,
) -> Result<(), &'static str> {
    let count = graph.nodes(wall).len();
    if index == 0 || index + 1 >= count {
        return Err("los extremos no se quitan");
    }
    if count <= THICKNESS_NODES.0 {
        return Err("mínimo 2 nodos");
    }
    Ok(())
}

/// Añade un nodo sobre la curva en x y lo apunta en el historial.
fn add_node(
    inputs: &mut Inputs,
    history: &mut BetaHistory,
    graph: &ThicknessGraph,
    wall: ThicknessWall,
    x: f64,
    time: f64,
) -> bool {
    let Some(setup) = graph.setup.with_node_added(&graph.curves, wall, x) else {
        return false;
    };
    inputs.blade.thickness = setup;
    history.record(&BetaSnapshot::of(inputs), time);
    true
}

/// Quita el nodo `index` y lo apunta en el historial.
fn remove_node(
    inputs: &mut Inputs,
    history: &mut BetaHistory,
    graph: &ThicknessGraph,
    wall: ThicknessWall,
    index: usize,
    time: f64,
) -> bool {
    let Some(setup) = graph.setup.with_node_removed(&graph.curves, wall, index) else {
        return false;
    };
    inputs.blade.thickness = setup;
    history.record(&BetaSnapshot::of(inputs), time);
    true
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
        status.push_str(&format!(" (mínimo de fundición: {} mm)", mm_trimmed(floor)));
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

    /// B1 con el nodo de salida (x = 1) de la curva puesto en `exit_mm`.
    fn edited_exit(exit_mm: f64) -> Inputs {
        let graph = graph_of(BladeThickness::default());
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free3D;
        inputs.blade.streamlines = 5;
        inputs.blade.thickness = graph.setup.with_node_moved(
            &graph.curves,
            ThicknessWall::Hub,
            2,
            1.0,
            exit_mm / 1000.0,
        );
        inputs
    }

    #[test]
    fn what_is_shown_is_what_is_saved() {
        let basis = Basis::default();
        let (gulich, _) = b1_3d();
        let inputs = edited_exit(6.0);
        let shown = crate::model::compute(&inputs, &basis).unwrap();
        // Un borde de salida más grueso mueve el 1D: β2 ya no es el de Gülich.
        assert!(shown.result.blade_edge_thickness.is_some());
        assert!(
            (shown.result.selection.beta2_deg - gulich.result.selection.beta2_deg).abs() > 1.0e-3,
            "β2 no se movió: {}",
            shown.result.selection.beta2_deg
        );

        // Lo guardado, recalculado, da lo mismo que se ve.
        let built = crate::model::build_project(&inputs, &basis, None, "B1").unwrap();
        let case = hm_engine::case_from_project(&built.project).unwrap();
        assert_eq!(
            case.design.blade_edge_thickness,
            shown.result.blade_edge_thickness
        );
        let again = hm_engine::analyze(&case, hm_engine::CurvePoints::new(11).unwrap()).unwrap();
        assert!(
            (again.result.selection.beta2_deg - shown.result.selection.beta2_deg).abs() < 1.0e-3,
            "{} frente a {}",
            again.result.selection.beta2_deg,
            shown.result.selection.beta2_deg
        );
        assert!(
            (again.result.beta1b - shown.result.beta1b).abs() < 1.0e-9,
            "{} frente a {}",
            again.result.beta1b,
            shown.result.beta1b
        );
    }

    #[test]
    fn gulich_gives_no_edge_data() {
        let (outcome, _) = b1_3d();
        assert_eq!(outcome.result.blade_edge_thickness, None);
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

    // ----- Tarea 4: añadir, quitar y escribir la cota -----

    #[test]
    fn written_millimetres_take_comma_or_point() {
        for text in ["4,5", "4.5", " 4,5 mm ", "4,5mm", "4,5 MM"] {
            assert_eq!(parse_thickness_mm(text), Ok(4.5), "{text:?}");
        }
    }

    #[test]
    fn an_empty_or_garbled_value_is_refused_with_a_reason() {
        for text in ["", "  ", "abc", "NaN", "-2", "inf", "0", "4,5,6"] {
            let reason = parse_thickness_mm(text).expect_err(text);
            assert!(!reason.is_empty(), "{text:?}");
        }
        // Y el nodo no cambia.
        let graph = graph_of(BladeThickness::default());
        assert!(write_thickness(&graph, ThicknessWall::Hub, 0, "abc").is_err());
        assert!(write_thickness(&graph, ThicknessWall::Hub, 9, "4,5").is_err());
    }

    #[test]
    fn a_written_value_under_the_minimum_is_raised_to_it() {
        let graph = graph_of(BladeThickness::default());
        let (setup, note) = write_thickness(&graph, ThicknessWall::Hub, 0, "1").unwrap();
        let after = graph_of(setup);
        assert!((after.nodes(ThicknessWall::Hub)[0][1] - 3.0).abs() < 1e-9);
        assert_eq!(note.as_deref(), Some("mínimo de fundición: 3 mm"));
        // El nodo de salida baja hasta 1,5 mm.
        let (setup, note) = write_thickness(&graph, ThicknessWall::Hub, 2, "1").unwrap();
        let after = graph_of(setup);
        assert!((after.nodes(ThicknessWall::Hub)[2][1] - 1.5).abs() < 1e-9);
        assert_eq!(note.as_deref(), Some("mínimo de fundición: 1,5 mm"));
        // Un valor que vale no lleva nota y no mueve la x del nodo.
        let (setup, note) = write_thickness(&graph, ThicknessWall::Hub, 1, "4,5").unwrap();
        let after = graph_of(setup);
        assert_eq!(note, None);
        assert_eq!(
            after.nodes(ThicknessWall::Hub)[1][0],
            graph.nodes(ThicknessWall::Hub)[1][0]
        );
        assert!((after.nodes(ThicknessWall::Hub)[1][1] - 4.5).abs() < 1e-9);
    }

    #[test]
    fn adding_a_node_keeps_the_curve_through_the_old_nodes() {
        let (outcome, _) = b1_3d();
        let graph = graph_of(BladeThickness::default());
        let mut inputs = outcome.inputs.clone();
        let mut history = BetaHistory::default();
        assert!(add_node(
            &mut inputs,
            &mut history,
            &graph,
            ThicknessWall::Hub,
            0.5,
            1.0
        ));
        let after = graph_of(inputs.blade.thickness.clone());
        let nodes = after.nodes(ThicknessWall::Hub);
        assert_eq!(nodes.len(), 4);
        for old in graph.nodes(ThicknessWall::Hub) {
            assert!(nodes.contains(&old), "{old:?} sigue en la curva");
        }
        // El nodo nuevo cae sobre la curva de antes.
        let new = nodes.iter().find(|n| n[0] == 0.5).expect("nodo en x = 0,5");
        assert!((new[1] - 1000.0 * graph.curves.hub.at(0.5)).abs() < 1e-9);
        assert_eq!(state_label(&after.setup), "editada");
    }

    #[test]
    fn the_menu_refuses_the_ninth_node_and_the_end_nodes() {
        let graph = graph_of(BladeThickness::default());
        let hub = ThicknessWall::Hub;
        assert_eq!(can_add(&graph, hub, 0.5), Ok(()));
        assert!(can_add(&graph, hub, 0.0).is_err());
        assert!(can_add(&graph, hub, 1.0).is_err());
        assert!(can_add(&graph, hub, 0.935).is_err(), "encima de otro nodo");
        assert!(can_remove(&graph, hub, 0).is_err());
        assert!(can_remove(&graph, hub, 2).is_err());
        assert_eq!(can_remove(&graph, hub, 1), Ok(()));
        // Con 8 nodos no se añade el noveno.
        let mut graph = graph;
        for x in [0.1, 0.2, 0.3, 0.4, 0.5] {
            let setup = graph
                .setup
                .with_node_added(&graph.curves, hub, x)
                .expect("cabe");
            graph = graph_of(setup);
        }
        assert_eq!(graph.nodes(hub).len(), 8);
        assert_eq!(can_add(&graph, hub, 0.7), Err("máximo 8 nodos"));
        // Con dos nodos solo quedan los extremos.
        let two = graph_of(BladeThickness::default());
        let two = two.setup.with_node_removed(&two.curves, hub, 1).unwrap();
        let two = graph_of(two);
        assert_eq!(two.nodes(hub).len(), 2);
        assert!(can_remove(&two, hub, 0).is_err());
        assert!(can_remove(&two, hub, 1).is_err());
    }

    #[test]
    fn adding_and_removing_are_undoable() {
        let (outcome, _) = b1_3d();
        let graph = graph_of(BladeThickness::default());
        let hub = ThicknessWall::Hub;
        let mut history = BetaHistory::default();
        history.follow(&BetaSnapshot::of(&outcome.inputs));
        let mut inputs = outcome.inputs.clone();
        assert!(add_node(&mut inputs, &mut history, &graph, hub, 0.5, 1.0));
        let previous = history.undo(&BetaSnapshot::of(&inputs)).unwrap();
        previous.apply(&mut inputs);
        assert_eq!(inputs.blade.thickness, BladeThickness::default());
        let next = history.redo(&BetaSnapshot::of(&inputs)).unwrap();
        next.apply(&mut inputs);
        let with_four = graph_of(inputs.blade.thickness.clone());
        assert_eq!(with_four.nodes(hub).len(), 4);
        // Quitar también se deshace.
        assert!(remove_node(
            &mut inputs,
            &mut history,
            &with_four,
            hub,
            1,
            3.0
        ));
        assert_eq!(graph_of(inputs.blade.thickness.clone()).nodes(hub).len(), 3);
        let previous = history.undo(&BetaSnapshot::of(&inputs)).unwrap();
        previous.apply(&mut inputs);
        assert_eq!(graph_of(inputs.blade.thickness.clone()).nodes(hub).len(), 4);
    }

    /// 400 px por unidad de x y 20 px por mm, con y hacia abajo.
    fn to_screen(p: [f64; 2]) -> egui::Pos2 {
        egui::pos2((p[0] * 400.0) as f32, (300.0 - p[1] * 20.0) as f32)
    }

    #[test]
    fn the_pointer_finds_a_node_a_curve_point_or_nothing() {
        let graph = graph_of(BladeThickness::default());
        let hub = ThicknessWall::Hub;
        let node = graph.nodes(hub)[2];
        let near = to_screen(node) + egui::vec2(3.0, 2.0);
        assert_eq!(target_at(&graph, &to_screen, near), Target::Node(hub, 2));
        let on_curve = to_screen([0.5, graph.curves.hub.at(0.5) * 1000.0]) + egui::vec2(0.0, 4.0);
        match target_at(&graph, &to_screen, on_curve) {
            Target::Curve(wall, x) => {
                assert_eq!(wall, hub);
                assert!((x - 0.5).abs() < 0.02, "x = {x}");
            }
            other => panic!("se esperaba la curva y salió {other:?}"),
        }
        let far = to_screen([0.5, 1.0]);
        assert_eq!(target_at(&graph, &to_screen, far), Target::Nothing);
    }

    #[test]
    fn a_double_click_on_the_label_or_the_node_edits_that_node() {
        let graph = graph_of(BladeThickness::default());
        let hub = ThicknessWall::Hub;
        let node = to_screen(graph.nodes(hub)[0]);
        // La cota del nodo 0 va a su derecha y arriba.
        let on_label = node + egui::vec2(20.0, -8.0);
        assert_eq!(label_hit(&graph, &to_screen, on_label), Some((hub, 0)));
        assert_eq!(label_hit(&graph, &to_screen, node), Some((hub, 0)));
        assert_eq!(
            label_hit(&graph, &to_screen, node + egui::vec2(0.0, 120.0)),
            None
        );
    }

    #[test]
    fn the_view_draws_with_the_edit_box_open() {
        let (outcome, camber) = b1_3d();
        let mut inputs = outcome.inputs.clone();
        let mut history = BetaHistory::default();
        let ctx_id = egui::Id::unique("blade_thickness_editing");
        egui::__run_test_ui(|ui| {
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    ctx_id,
                    Editing {
                        wall: ThicknessWall::Hub,
                        index: 0,
                        text: "4,5".to_string(),
                        opened: false,
                    },
                );
            });
            thickness_view(ui, &outcome, &mut inputs, &mut history, &camber);
        });
    }
}
