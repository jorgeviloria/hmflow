//! Pestaña Meridiano: el corte meridional, dibujado desde `MeridionalModel`,
//! con cada tapa como Bézier de 5 puntos que se edita con cajas o arrastrando
//! P1, P2 y P3: P2 dentro del rectángulo de los extremos y los brazos de
//! inicio y final con su ángulo y su longitud.

use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Legend, Plot, PlotPoint, Points};

use crate::curves::line;
use hm_design::meridian::{
    AUTO_END_ANGLE, AUTO_START_ANGLE, ChannelStation, ControlPoint, MeridianEdits, MeridionalModel,
    Moved, Wall, meridional_model, meridional_model_with,
};
use hm_project::{Meridian, MeridianWall};

use crate::model::Outcome;

/// Color de cada curva del corte. Una curva sin color asignado se dibuja
/// igual, en gris: no desaparece.
fn meridian_color(name: &str) -> Color32 {
    match name {
        "Tapa delantera" => Color32::from_rgb(0, 128, 0),
        "Tapa trasera" => Color32::from_rgb(30, 30, 255),
        "Borde de ataque" => Color32::from_rgb(20, 20, 20),
        "Borde de salida" => Color32::from_rgb(70, 70, 70),
        _ => Color32::GRAY,
    }
}

/// Distancia en píxeles a la que un clic agarra un punto de control.
const GRAB_PIXELS: f32 = 12.0;

/// Una cota sobre el dibujo: línea de `from` a `to` con su texto.
#[derive(Debug, Clone, PartialEq)]
struct Dimension {
    from: [f64; 2],
    to: [f64; 2],
    text: String,
    anchor: egui::Align2,
}

/// Las cotas del corte, en mm: zE y b2 sobre la salida, cada una a su altura
/// para que los textos no se pisen, y la anchura de entrada b1 paralela al
/// borde de ataque, por fuera del círculo de la entrada. r2 y r1 están en la
/// cabecera: una cota hasta el eje encogería el dibujo.
fn dimensions(model: &MeridionalModel) -> Vec<Dimension> {
    let p = &model.params;
    let r2 = p.shroud.points[4][1];
    let off = 0.04 * r2;
    let b2 = p.hub.points[4][0] - p.z_e;
    let mut dimensions = vec![
        Dimension {
            from: [0.0, r2 + off],
            to: [p.z_e, r2 + off],
            text: format!("zE = {:.1}", p.z_e),
            anchor: egui::Align2::CENTER_BOTTOM,
        },
        Dimension {
            from: [p.z_e, r2 + 3.0 * off],
            to: [p.z_e + b2, r2 + 3.0 * off],
            text: format!("b2 = {b2:.1}"),
            anchor: egui::Align2::CENTER_BOTTOM,
        },
    ];
    if let Some(edge) = model.curve("Borde de ataque") {
        let (top, bottom) = (edge.points[0], edge.points[1]);
        // El círculo de la entrada tiene por diámetro el borde: sobresale hacia
        // atrás su radio.
        let back = p.inlet_width / 2.0 + 2.0 * off;
        dimensions.push(Dimension {
            from: [top[0] - back, top[1]],
            to: [bottom[0] - back, bottom[1]],
            text: format!("b1 = {:.1}", p.inlet_width),
            anchor: egui::Align2::RIGHT_CENTER,
        });
    }
    dimensions
}

/// Cuántos círculos de área se dibujan, repartidos por la línea media.
const AREA_CIRCLES: usize = 24;

/// Rango de RDs de Gülich, en veces b1 (ec. 7.2, p. 416).
const RDS_RANGE: (f64, f64) = (0.6, 0.8);

/// Pasos que se recuerdan para deshacer.
const HISTORY_STEPS: usize = 200;

/// Cambios seguidos a menos de esto (segundos) forman un solo paso: un
/// arrastre o una cifra tras otra se deshacen de una vez.
const HISTORY_GAP: f64 = 0.6;

/// Deshacer y rehacer de lo editado en el meridiano. Guarda copias de
/// `[meridian]`; si el archivo se abre o se cambia desde fuera, empieza de nuevo.
#[derive(Debug, Default)]
pub(super) struct History {
    /// Lo que había al final del fotograma anterior.
    last: Meridian,
    undo: Vec<Meridian>,
    redo: Vec<Meridian>,
    /// Instante, en segundos, del último cambio.
    last_change: f64,
}

impl History {
    /// Lo editado cambió por otra vía que no es esta pestaña: se olvida lo anterior.
    fn follow(&mut self, now: &Meridian) {
        if *now != self.last {
            self.last = now.clone();
            self.undo.clear();
            self.redo.clear();
        }
    }

    /// Anota un cambio hecho en la pestaña, en el instante `time`.
    fn record(&mut self, now: &Meridian, time: f64) {
        if time - self.last_change > HISTORY_GAP || self.undo.is_empty() {
            self.undo.push(self.last.clone());
            if self.undo.len() > HISTORY_STEPS {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_change = time;
        self.last = now.clone();
    }

    fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Vuelve al paso anterior; `false` si no hay.
    fn undo(&mut self, boxes: &mut Meridian) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(boxes, previous));
        self.last = boxes.clone();
        self.last_change = f64::NEG_INFINITY;
        true
    }

    /// Rehace el paso deshecho; `false` si no hay.
    fn redo(&mut self, boxes: &mut Meridian) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(boxes, next));
        self.last = boxes.clone();
        self.last_change = f64::NEG_INFINITY;
        true
    }
}

/// La pestaña con deshacer y rehacer (botones y Cmd/Ctrl+Z, Mayús+Cmd+Z o
/// Cmd+Y; los atajos no actúan mientras se escribe en una caja, que tiene el suyo).
pub(super) fn meridian_view(
    ui: &mut egui::Ui,
    outcome: &Outcome,
    boxes: &mut Meridian,
    history: &mut History,
) -> bool {
    use egui::{Key, KeyboardShortcut, Modifiers};
    history.follow(boxes);
    let typing = ui.memory(|memory| memory.focused().is_some());
    let (redo_keys, undo_key) = ui.input_mut(|input| {
        let redo = !typing
            && (input.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Z,
            )) || input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y)));
        let undo =
            !typing && input.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z));
        (redo, undo)
    });
    let mut changed = false;
    ui.horizontal(|ui| {
        let undo = ui.add_enabled(history.can_undo(), egui::Button::new("Deshacer"));
        let redo = ui.add_enabled(history.can_redo(), egui::Button::new("Rehacer"));
        if (undo.clicked() || undo_key) && history.undo(boxes) {
            changed = true;
        }
        if (redo.clicked() || redo_keys) && history.redo(boxes) {
            changed = true;
        }
    });
    if meridian_edit(ui, outcome, boxes) {
        history.record(boxes, ui.input(|input| input.time));
        changed = true;
    }
    changed
}

/// Edita `[meridian]` en `boxes` con texto o arrastrando los puntos de
/// control. Devuelve si cambió algo.
fn meridian_edit(ui: &mut egui::Ui, outcome: &Outcome, boxes: &mut Meridian) -> bool {
    let auto = meridional_model(&outcome.result);
    let mut changed = wall_boxes(ui, boxes, auto.as_ref().ok());
    let edits = hm_engine::meridian_edits_of(boxes);
    // El visor no calcula: dibuja el mismo modelo que se exporta.
    let model = match meridional_model_with(&outcome.result, &edits) {
        Ok(model) => model,
        Err(error) => {
            ui.label(
                RichText::new(format!("Sin meridiano: {error}."))
                    .color(ui.visuals().error_fg_color),
            );
            return changed;
        }
    };
    ui.label(RichText::new(header(&model, &edits)).small().weak());
    let engine = (edits != MeridianEdits::default())
        .then_some(auto.ok())
        .flatten();
    let graphs_height = (ui.available_height() * 0.35).max(140.0);
    Plot::new("meridian")
        .data_aspect(1.0)
        .legend(Legend::default())
        .allow_drag(false)
        .height(ui.available_height() - graphs_height)
        .show(ui, |plot_ui| {
            if let Some(engine) = &engine {
                for wall in Wall::ALL {
                    if let Some(curve) = engine.curve(wall.name()) {
                        plot_ui.line(
                            line("Propuesta del motor", curve.points.clone(), Color32::GRAY)
                                .width(1.0)
                                .style(egui_plot::LineStyle::dashed_loose()),
                        );
                    }
                }
            }
            for curve in &model.curves {
                let color = meridian_color(&curve.name);
                plot_ui.line(line(&curve.name, curve.points.clone(), color));
            }
            // El tope de P2: el rectángulo que forman P0 y P4.
            for wall in Wall::ALL {
                let (low, high) = model.params.wall(wall).p2_range;
                plot_ui.line(
                    line(
                        "Tope de P2",
                        vec![
                            [low[0], low[1]],
                            [high[0], low[1]],
                            [high[0], high[1]],
                            [low[0], high[1]],
                            [low[0], low[1]],
                        ],
                        Color32::from_gray(90),
                    )
                    .width(1.0)
                    .style(egui_plot::LineStyle::dotted_dense()),
                );
            }
            // El polígono de control de cada tapa, P0 a P4.
            for wall in Wall::ALL {
                plot_ui.line(
                    line(
                        "Puntos de control",
                        model.params.wall(wall).points.to_vec(),
                        meridian_color(wall.name()).gamma_multiply(0.6),
                    )
                    .width(1.0)
                    .style(egui_plot::LineStyle::dashed_dense()),
                );
            }
            for wall in Wall::ALL {
                let colour = meridian_color(wall.name());
                plot_ui.points(
                    Points::new(
                        "Puntos de control",
                        model
                            .handles()
                            .iter()
                            .filter(|handle| handle.control.wall == wall)
                            .map(|handle| handle.at)
                            .collect::<Vec<_>>(),
                    )
                    .radius(5.0)
                    .color(colour),
                );
            }
            // La curvatura máxima de cada tapa: donde está su radio mínimo.
            for wall in Wall::ALL {
                let shape = model.params.wall(wall);
                plot_ui.points(
                    Points::new("Curvatura máxima", vec![shape.max_curvature_at])
                        .radius(7.0)
                        .shape(egui_plot::MarkerShape::Diamond)
                        .color(Color32::from_rgb(220, 60, 60))
                        .filled(false),
                );
            }
            // Las cotas.
            for dimension in dimensions(&model) {
                let grey = Color32::from_gray(90);
                plot_ui.line(line("Cotas", vec![dimension.from, dimension.to], grey).width(0.8));
                plot_ui.text(
                    egui_plot::Text::new(
                        "Cotas",
                        PlotPoint::new(
                            (dimension.from[0] + dimension.to[0]) / 2.0,
                            (dimension.from[1] + dimension.to[1]) / 2.0,
                        ),
                        RichText::new(dimension.text).small(),
                    )
                    .anchor(dimension.anchor)
                    .color(grey),
                );
            }
            // Los círculos de área, como en CFturbo: la familia en gris, el de la
            // entrada y el de la salida en rojo y la línea de sus centros.
            let circles = area_circles(&model, AREA_CIRCLES);
            let red = Color32::from_rgb(220, 60, 60);
            for (index, (centre, radius)) in circles.iter().enumerate() {
                let end = index == 0 || index + 1 == circles.len();
                plot_ui.line(
                    line(
                        if end {
                            "Círculos de entrada y salida"
                        } else {
                            "Círculos de área"
                        },
                        circle_points(*centre, *radius),
                        if end { red } else { Color32::from_gray(150) },
                    )
                    .width(if end { 1.5 } else { 0.7 }),
                );
            }
            plot_ui.line(
                line(
                    "Línea de centros",
                    circles.iter().map(|(centre, _)| *centre).collect(),
                    red,
                )
                .width(1.0)
                .style(egui_plot::LineStyle::dashed_dense()),
            );
            changed |= drag_handles(plot_ui, &model, boxes);
        });
    channel_graphs(ui, &model, outcome.result.qth.si);
    changed
}

/// La cabecera: cotas, radios mínimos frente a RDs y lo editado.
fn header(model: &MeridionalModel, edits: &MeridianEdits) -> String {
    let p = &model.params;
    let (low, high) = (RDS_RANGE.0 * p.inlet_width, RDS_RANGE.1 * p.inlet_width);
    let shroud = p.shroud.min_radius;
    let verdict = if shroud < low {
        format!(
            " por debajo de {low:.1}–{high:.1} (0,6–0,8·b1, ec. 7.2): con zE = {:.1} no cabe más",
            p.z_e
        )
    } else {
        format!(" (Gülich: {low:.1}–{high:.1})")
    };
    format!(
        "Corte meridional, mm: z con el flujo (0 en el borde de ataque), r radial. zE = {:.1}, \
         d1i = {:.1}, anchura de entrada = {:.1}. Borde de ataque: {:.1}° con la radial, longitud {:.1}, \
         área {:.0} mm². Radio mínimo: tapa delantera {shroud:.1}{verdict}; \
         trasera {:.1}. Cada tapa es una Bézier de 5 puntos: P2 se mueve dentro del rectángulo de los \
         extremos y P1 y P3 salen de ellos con su ángulo (° desde +z hacia +r) y su longitud. Gülich §7.2.2.1.{} El álabe sigue siendo 2D hasta F4. Huella {}.",
        p.z_e,
        p.d1i,
        p.inlet_width,
        p.edge_angle,
        p.edge_length,
        p.edge_area,
        p.hub.min_radius,
        edited_note(edits),
        model.fingerprint_text()
    )
}

/// Las cajas de las dos tapas, con lo del motor como pista. Vacía es el
/// motor.
fn wall_boxes(ui: &mut egui::Ui, boxes: &mut Meridian, auto: Option<&MeridionalModel>) -> bool {
    let mut changed = false;
    let parsed = hm_engine::meridian_edits_of(boxes);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Borde de ataque").strong());
        ui.label(RichText::new("εEK °").small());
        let bad = parsed.edge_angle.is_none() && !boxes.edge_angle_deg.trim().is_empty();
        let mut field = egui::TextEdit::singleline(&mut boxes.edge_angle_deg)
            .id_salt("edge_angle")
            .hint_text(format!("{} (auto)", angle_text(0.0)))
            .desired_width(100.0);
        if bad {
            field = field.text_color(ui.visuals().error_fg_color);
        }
        changed |= ui
            .add(field)
            .on_hover_text(
                "Inclinación de la recta cubo–tapa con la radial, de 0 a 60 grados: el cubo \
                 empieza aguas abajo de la tapa. El área de entrada sobre esa recta sigue \
                 siendo la del 1D. Vacía: 0.",
            )
            .changed();
    });
    for wall in Wall::ALL {
        let shape = auto.map(|model| model.params.wall(wall).clone());
        let edit = *parsed.wall(wall);
        let texts = wall_mut(boxes, wall);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(wall.name()).strong());
            let hints = shape
                .as_ref()
                .map(|shape| {
                    [
                        format!("{} (auto)", length_text(shape.points[2][0])),
                        format!("{} (auto)", length_text(shape.points[2][1])),
                        format!("{} (auto)", angle_text(AUTO_START_ANGLE)),
                        format!("{} (auto)", length_text(shape.auto_start_length)),
                        format!("{} (auto)", angle_text(AUTO_END_ANGLE)),
                        format!("{} (auto)", length_text(shape.auto_end_length)),
                    ]
                })
                .unwrap_or_default();
            let fields: [(&str, &mut String, bool); 6] = [
                ("P2 z", &mut texts.p2_z, edit.p2.is_none()),
                ("P2 r", &mut texts.p2_r, edit.p2.is_none()),
                (
                    "Inicio °",
                    &mut texts.start_angle_deg,
                    edit.start_angle.is_none(),
                ),
                (
                    "Inicio L",
                    &mut texts.start_length,
                    edit.start_length.is_none(),
                ),
                ("Final °", &mut texts.end_angle_deg, edit.end_angle.is_none()),
                ("Final L", &mut texts.end_length, edit.end_length.is_none()),
            ];
            for (index, (label, text, unread)) in fields.into_iter().enumerate() {
                let bad = unread && !text.trim().is_empty();
                ui.label(RichText::new(label).small());
                let mut field = egui::TextEdit::singleline(text)
                    .id_salt((wall.file_name(), label))
                    .hint_text(hints[index].as_str())
                    .desired_width(100.0);
                if bad {
                    field = field.text_color(ui.visuals().error_fg_color);
                }
                changed |= ui
                    .add(field)
                    .on_hover_text("P2: longitud con unidad, como 24 mm, dentro del rectángulo de los extremos. \
                         Ángulo: grados de -90 a 90 desde +z hacia +r (0 axial, 90 radial). \
                         Longitud del brazo: con unidad, mayor que 0. Vacía: el motor.")
                    .changed();
            }
        });
    }
    let any = !boxes.is_empty();
    if ui
        .add_enabled(any, egui::Button::new("Todo del motor"))
        .clicked()
    {
        *boxes = Meridian::default();
        changed = true;
    }
    changed
}

/// Agarra el punto de control más cercano al empezar a arrastrar y, mientras
/// dura, escribe en sus cajas lo que da el puntero.
fn drag_handles(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    boxes: &mut Meridian,
) -> bool {
    let id = egui::Id::unique("meridian_dragged_point");
    let response = plot_ui.response().clone();
    let Some(pointer) = plot_ui.pointer_coordinate() else {
        return false;
    };
    if response.drag_started() {
        let screen = plot_ui.screen_from_plot(pointer);
        let handles: Vec<(ControlPoint, [f32; 2])> = model
            .handles()
            .iter()
            .map(|handle| {
                let at = plot_ui.screen_from_plot(PlotPoint::new(handle.at[0], handle.at[1]));
                (handle.control, [at.x, at.y])
            })
            .collect();
        let grabbed = nearest(&handles, [screen.x, screen.y], GRAB_PIXELS);
        plot_ui.ctx().data_mut(|data| data.insert_temp(id, grabbed));
    }
    let grabbed: Option<ControlPoint> = plot_ui.ctx().data(|data| data.get_temp(id)).flatten();
    if response.drag_stopped() {
        plot_ui
            .ctx()
            .data_mut(|data| data.remove::<Option<ControlPoint>>(id));
    }
    let (Some(control), true) = (grabbed, response.dragged()) else {
        return false;
    };
    write_moved(boxes, control, model.moved(control, [pointer.x, pointer.y]))
}

/// Escribe en las cajas lo que dio el arrastre. Devuelve si cambió algo.
fn write_moved(boxes: &mut Meridian, control: ControlPoint, moved: Moved) -> bool {
    let texts = wall_mut(boxes, control.wall);
    let updates: Vec<(&mut String, String)> = match (moved, control.index) {
        (Moved::Point([z, r]), _) => vec![
            (&mut texts.p2_z, length_text(z)),
            (&mut texts.p2_r, length_text(r)),
        ],
        (Moved::Arm { angle, length }, 1) => vec![
            (&mut texts.start_angle_deg, angle_text(angle)),
            (&mut texts.start_length, length_text(length)),
        ],
        (Moved::Arm { angle, length }, _) => vec![
            (&mut texts.end_angle_deg, angle_text(angle)),
            (&mut texts.end_length, length_text(length)),
        ],
    };
    let mut changed = false;
    for (target, text) in updates {
        if *target != text {
            *target = text;
            changed = true;
        }
    }
    changed
}

/// El punto más cercano a `pointer`, si está a menos de `tolerance`.
fn nearest(
    handles: &[(ControlPoint, [f32; 2])],
    pointer: [f32; 2],
    tolerance: f32,
) -> Option<ControlPoint> {
    handles
        .iter()
        .map(|(control, at)| (*control, (at[0] - pointer[0]).hypot(at[1] - pointer[1])))
        .filter(|(_, distance)| *distance <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(control, _)| control)
}

fn length_text(value: f64) -> String {
    format!("{value:.2} mm")
}

/// Un ángulo en grados, sin unidad (así lo guarda `[meridian]`).
fn angle_text(value: f64) -> String {
    format!("{value:.1}")
}

fn wall_mut(boxes: &mut Meridian, wall: Wall) -> &mut MeridianWall {
    match wall {
        Wall::Shroud => &mut boxes.shroud,
        Wall::Hub => &mut boxes.hub,
    }
}

/// Qué viene editado, para la cabecera.
fn edited_note(edits: &MeridianEdits) -> String {
    let mut names = Vec::new();
    if edits.edge_angle.is_some() {
        names.push("borde de ataque".to_string());
    }
    for wall in Wall::ALL {
        let edit = edits.wall(wall);
        for (present, point) in [
            (edit.p2.is_some(), "P2"),
            (
                edit.start_angle.is_some() || edit.start_length.is_some(),
                "P1",
            ),
            (edit.end_angle.is_some() || edit.end_length.is_some(), "P3"),
        ] {
            if present {
                names.push(format!("{point} {}", wall.name().to_lowercase()));
            }
        }
    }
    if names.is_empty() {
        String::new()
    } else {
        format!(" Editado: {}.", names.join(", "))
    }
}

/// A(m) y cm(m) sobre la línea media: geometría del corte, sin bloqueo de
/// álabes. No predicen rendimiento.
fn channel_graphs(ui: &mut egui::Ui, model: &MeridionalModel, qth: f64) {
    let channel = model.channel();
    let (_, velocity) = profiles(&channel, qth);
    let (area, reference) = area_against_length(&channel);
    ui.label(
        RichText::new(
            "Sobre la línea media. A = 2π·r·b frente a M/Mmax (fracción de la longitud de la línea media), \
             con la recta de la entrada a la salida como referencia; cm = Qth/A con m en mm desde el \
             borde de ataque. Sin bloqueo de álabes: es geometría del corte, no una predicción de \
             rendimiento.",
        )
        .small()
        .weak(),
    );
    ui.columns(2, |columns| {
        Plot::new("channel_area")
            .legend(Legend::default())
            .show(&mut columns[0], |plot_ui| {
                plot_ui.line(line("A, cm²", area, Color32::from_rgb(70, 150, 240)));
                plot_ui.line(
                    line("Recta entrada–salida", reference, Color32::GRAY)
                        .width(1.0)
                        .style(egui_plot::LineStyle::dashed_loose()),
                );
            });
        Plot::new("channel_cm")
            .legend(Legend::default())
            .show(&mut columns[1], |plot_ui| {
                plot_ui.line(line("cm, m/s", velocity, Color32::from_rgb(240, 160, 50)));
            });
    });
}

/// Puntos de A(m) en cm² y de cm(m) en m/s.
fn profiles(channel: &[ChannelStation], qth: f64) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let area = channel
        .iter()
        .map(|station| [station.m, station.area / 100.0])
        .collect();
    let velocity = channel
        .iter()
        .map(|station| [station.m, station.cm(qth)])
        .collect();
    (area, velocity)
}

/// Punto de la polilínea más cercano a `c` y su distancia.
fn nearest_on(polyline: &[[f64; 2]], c: [f64; 2]) -> ([f64; 2], f64) {
    let mut best = (polyline[0], f64::INFINITY);
    for pair in polyline.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length_squared = dx * dx + dy * dy;
        let t = if length_squared > 0.0 {
            (((c[0] - a[0]) * dx + (c[1] - a[1]) * dy) / length_squared).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let point = [a[0] + t * dx, a[1] + t * dy];
        let distance = (point[0] - c[0]).hypot(point[1] - c[1]);
        if distance < best.1 {
            best = (point, distance);
        }
    }
    best
}

/// `count` círculos de área, centro y radio, de la entrada a la salida, con
/// los dos extremos incluidos (Gülich, p. 417; igual que el editor de CFturbo):
/// el centro está en la cuerda de tapa a tapa de la estación, a igual distancia
/// de las dos, y el radio es esa distancia, así que cada círculo es tangente a
/// la tapa delantera y a la trasera. Los bordes no lo limitan: el de la entrada
/// puede sobresalir del borde de ataque.
fn area_circles(model: &MeridionalModel, count: usize) -> Vec<([f64; 2], f64)> {
    const ITERATIONS: usize = 50;
    let channel = model.channel();
    let (Some(shroud), Some(hub)) = (
        model.curve(Wall::Shroud.name()),
        model.curve(Wall::Hub.name()),
    ) else {
        return Vec::new();
    };
    if channel.len() < 2 || count < 2 {
        return Vec::new();
    }
    let last = channel.len() - 1;
    let mut picked: Vec<usize> = (0..count)
        .map(|k| (k * last + (count - 1) / 2) / (count - 1))
        .collect();
    picked.dedup();
    picked
        .into_iter()
        .map(|index| {
            let station = &channel[index];
            let (from, to) = (station.shroud_point, station.hub_point);
            let at = |u: f64| {
                [
                    from[0] + (to[0] - from[0]) * u,
                    from[1] + (to[1] - from[1]) * u,
                ]
            };
            // Sobre la cuerda de tapa a tapa, el punto equidistante de las dos.
            let balance = |u: f64| {
                let centre = at(u);
                nearest_on(&shroud.points, centre).1 - nearest_on(&hub.points, centre).1
            };
            let (mut low, mut high) = (0.0, 1.0);
            for _ in 0..ITERATIONS {
                let middle = (low + high) / 2.0;
                if balance(middle) < 0.0 {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            let centre = at((low + high) / 2.0);
            let radius = nearest_on(&shroud.points, centre)
                .1
                .min(nearest_on(&hub.points, centre).1);
            (centre, radius)
        })
        .collect()
}

/// La circunferencia como polilínea cerrada.
fn circle_points(centre: [f64; 2], radius: f64) -> Vec<[f64; 2]> {
    const SEGMENTS: usize = 64;
    let mut points: Vec<[f64; 2]> = (0..SEGMENTS)
        .map(|step| {
            let angle = std::f64::consts::TAU * step as f64 / SEGMENTS as f64;
            [
                centre[0] + radius * angle.cos(),
                centre[1] + radius * angle.sin(),
            ]
        })
        .collect();
    points.push(points[0]);
    points
}

/// A(M/Mmax) en cm² y la recta de referencia de la entrada a la salida. Sin
/// estaciones, o con una línea media de longitud nula, no hay gráfica.
fn area_against_length(channel: &[ChannelStation]) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let (Some(first), Some(last)) = (channel.first(), channel.last()) else {
        return (Vec::new(), Vec::new());
    };
    let total = last.m - first.m;
    if total <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    let area = channel
        .iter()
        .map(|station| [(station.m - first.m) / total, station.area / 100.0])
        .collect();
    let reference = vec![[0.0, first.area / 100.0], [1.0, last.area / 100.0]];
    (area, reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_part_of_the_section_has_its_own_colour() {
        let names = [
            "Tapa delantera",
            "Tapa trasera",
            "Borde de ataque",
            "Borde de salida",
        ];
        let colours: Vec<Color32> = names.iter().map(|name| meridian_color(name)).collect();
        for (index, colour) in colours.iter().enumerate() {
            assert_ne!(*colour, Color32::GRAY, "{}", names[index]);
            assert!(!colours[..index].contains(colour), "{}", names[index]);
        }
    }

    #[test]
    fn a_curve_without_a_colour_is_still_drawn() {
        assert_eq!(meridian_color("Nariz del cubo"), Color32::GRAY);
    }

    #[test]
    fn the_graphs_are_the_channel_in_cm2_and_m_per_s() {
        let station = ChannelStation {
            m: 12.0,
            z: 3.0,
            shroud_point: [3.0, 60.0],
            hub_point: [3.0, 40.0],
            r: 50.0,
            width: 20.0,
            area: 2.0 * std::f64::consts::PI * 50.0 * 20.0,
        };
        let (area, velocity) = profiles(std::slice::from_ref(&station), 0.01);
        assert_eq!(area, vec![[12.0, station.area / 100.0]]);
        assert_eq!(velocity, vec![[12.0, 0.01 / (station.area * 1e-6)]]);
    }

    #[test]
    fn the_header_names_what_is_edited() {
        assert_eq!(edited_note(&MeridianEdits::default()), "");
        let mut edits = MeridianEdits::default();
        edits.shroud.p2 = Some([20.0, 60.0]);
        edits.hub.end_length = Some(4.0);
        assert_eq!(
            edited_note(&edits),
            " Editado: P2 tapa delantera, P3 tapa trasera."
        );
    }

    #[test]
    fn a_click_grabs_the_nearest_point_within_reach() {
        let p = |wall, index| ControlPoint { wall, index };
        let handles = [
            (p(Wall::Shroud, 1), [100.0, 100.0]),
            (p(Wall::Shroud, 2), [110.0, 100.0]),
            (p(Wall::Hub, 2), [300.0, 300.0]),
        ];
        assert_eq!(
            nearest(&handles, [108.0, 101.0], 12.0),
            Some(p(Wall::Shroud, 2))
        );
        assert_eq!(nearest(&handles, [200.0, 200.0], 12.0), None);
    }

    #[test]
    fn a_drag_writes_the_point_or_the_arm_in_its_boxes() {
        let mut boxes = Meridian::default();
        let p2 = ControlPoint {
            wall: Wall::Hub,
            index: 2,
        };
        assert!(write_moved(&mut boxes, p2, Moved::Point([30.0, 25.5])));
        assert_eq!(
            (boxes.hub.p2_z.as_str(), boxes.hub.p2_r.as_str()),
            ("30.00 mm", "25.50 mm")
        );
        // Lo mismo otra vez no cambia nada.
        assert!(!write_moved(&mut boxes, p2, Moved::Point([30.0, 25.5])));

        let arm = Moved::Arm {
            angle: 33.34,
            length: 7.456,
        };
        let p3 = ControlPoint {
            wall: Wall::Shroud,
            index: 3,
        };
        assert!(write_moved(&mut boxes, p3, arm));
        assert_eq!(boxes.shroud.end_angle_deg, "33.3");
        assert_eq!(boxes.shroud.end_length, "7.46 mm");
        assert_eq!(boxes.shroud.start_angle_deg, "");
        let p1 = ControlPoint {
            wall: Wall::Shroud,
            index: 1,
        };
        assert!(write_moved(&mut boxes, p1, arm));
        assert_eq!(
            (
                boxes.shroud.start_angle_deg.as_str(),
                boxes.shroud.start_length.as_str()
            ),
            ("33.3", "7.46 mm")
        );
        let edits = hm_engine::meridian_edits_of(&boxes);
        let [z, r] = edits.hub.p2.unwrap();
        assert!((z - 30.0).abs() < 1e-9 && (r - 25.5).abs() < 1e-9);
        assert_eq!(edits.shroud.end_angle, Some(33.3));
        assert!((edits.shroud.end_length.unwrap() - 7.46).abs() < 1e-9);
    }

    #[test]
    fn the_header_compares_the_radius_with_guelich() {
        let outcome = crate::model::compute(
            &crate::model::Inputs::default(),
            &crate::model::Basis::default(),
        )
        .unwrap();
        let model = meridional_model(&outcome.result).unwrap();
        let text = header(&model, &MeridianEdits::default());
        let p = &model.params;
        let low = 0.6 * p.inlet_width;
        if p.shroud.min_radius < low {
            assert!(text.contains("por debajo"), "{text}");
        } else {
            assert!(text.contains("Gülich:"), "{text}");
        }
        assert!(text.contains(&model.fingerprint_text()));
    }

    #[test]
    fn the_area_graph_runs_from_0_to_1_with_a_straight_reference() {
        let station = |m: f64, area: f64| ChannelStation {
            m,
            z: 0.0,
            shroud_point: [0.0, 60.0],
            hub_point: [0.0, 40.0],
            r: 50.0,
            width: 20.0,
            area: area * 100.0,
        };
        let channel = [station(5.0, 40.0), station(25.0, 30.0), station(45.0, 20.0)];
        let (area, reference) = area_against_length(&channel);
        assert_eq!(area, [[0.0, 40.0], [0.5, 30.0], [1.0, 20.0]]);
        assert_eq!(reference, [[0.0, 40.0], [1.0, 20.0]]);
        assert_eq!(area_against_length(&[]), (Vec::new(), Vec::new()));
        assert_eq!(
            area_against_length(&[station(3.0, 1.0)]),
            (Vec::new(), Vec::new())
        );
    }

    #[test]
    fn the_area_circles_touch_both_walls_and_are_spread_from_inlet_to_outlet() {
        let result = hm_design::design(&hm_design::DesignInput {
            q: hm_units::parse_quantity("120 m3/h").unwrap(),
            h: hm_units::parse_quantity("45 m").unwrap(),
            n: hm_units::parse_quantity("2900 rpm").unwrap(),
            rho: hm_units::parse_quantity("998.2 kg/m3").unwrap(),
            eta_m: 0.95,
            npsha: None,
            nu: None,
        })
        .unwrap();
        let model = meridional_model(&result).unwrap();
        let circles = area_circles(&model, AREA_CIRCLES);
        assert_eq!(circles.len(), AREA_CIRCLES);
        // Repartidos por el corte, no amontonados en un punto.
        for pair in circles.windows(2) {
            let apart = (pair[0].0[0] - pair[1].0[0]).hypot(pair[0].0[1] - pair[1].0[1]);
            assert!(apart > 0.1, "{pair:?}");
        }
        // Ninguno es pequeño por culpa de un borde: el de la entrada abarca todo el ojo.
        let inlet = &circles[0];
        let edge = &model.curve("Borde de ataque").unwrap().points;
        let edge_length = (edge[0][0] - edge[1][0]).hypot(edge[0][1] - edge[1][1]);
        assert!(inlet.1 > edge_length * 0.4, "{} {edge_length}", inlet.1);
        for (centre, radius) in &circles {
            let shroud = nearest_on(&model.curve("Tapa delantera").unwrap().points, *centre).1;
            let hub = nearest_on(&model.curve("Tapa trasera").unwrap().points, *centre).1;
            // Dentro de las dos tapas y tangente a las dos.
            assert!((shroud - hub).abs() < 0.02 * radius, "{shroud} {hub}");
            assert!((radius - shroud.min(hub)).abs() < 1e-9);
        }
        let points = circle_points([1.0, 2.0], 3.0);
        assert_eq!(points.first(), points.last());
        assert!(
            points
                .iter()
                .all(|p| ((p[0] - 1.0).hypot(p[1] - 2.0) - 3.0).abs() < 1e-9)
        );
    }

    fn edits(start_angle: &str) -> Meridian {
        Meridian {
            shroud: MeridianWall {
                start_angle_deg: start_angle.into(),
                ..MeridianWall::default()
            },
            ..Meridian::default()
        }
    }

    #[test]
    fn a_burst_of_changes_is_one_undo_step_and_undo_and_redo_walk_it() {
        let mut history = History::default();
        let mut boxes = Meridian::default();
        history.follow(&boxes);
        // Un arrastre: tres cambios seguidos.
        for (time, value) in [(1.0, "10"), (1.1, "20"), (1.2, "30")] {
            boxes = edits(value);
            history.record(&boxes, time);
        }
        // Otro, más tarde.
        boxes = edits("45");
        history.record(&boxes, 5.0);
        assert_eq!(history.undo.len(), 2);

        assert!(history.undo(&mut boxes));
        assert_eq!(boxes, edits("30"));
        assert!(history.undo(&mut boxes));
        assert_eq!(boxes, Meridian::default());
        assert!(!history.can_undo());
        assert!(!history.undo(&mut boxes));

        assert!(history.redo(&mut boxes));
        assert_eq!(boxes, edits("30"));
        assert!(history.redo(&mut boxes));
        assert_eq!(boxes, edits("45"));
        assert!(!history.can_redo());
    }

    #[test]
    fn a_new_change_clears_redo_and_an_outside_change_starts_over() {
        let mut history = History::default();
        let mut boxes = edits("10");
        history.record(&boxes, 1.0);
        assert!(history.undo(&mut boxes));
        assert!(history.can_redo());
        boxes = edits("50");
        history.record(&boxes, 9.0);
        assert!(!history.can_redo());

        // Se abre otro archivo: lo editado cambia sin pasar por la pestaña.
        let opened = edits("-5");
        history.follow(&opened);
        assert!(!history.can_undo() && !history.can_redo());
    }

    #[test]
    fn the_dimensions_give_ze_b2_and_the_inlet_width_of_the_model() {
        let result = hm_design::design(&hm_design::DesignInput {
            q: hm_units::parse_quantity("120 m3/h").unwrap(),
            h: hm_units::parse_quantity("45 m").unwrap(),
            n: hm_units::parse_quantity("2900 rpm").unwrap(),
            rho: hm_units::parse_quantity("998.2 kg/m3").unwrap(),
            eta_m: 0.95,
            npsha: None,
            nu: None,
        })
        .unwrap();
        let model = meridional_model(&result).unwrap();
        let dimensions = dimensions(&model);
        let p = &model.params;
        let by = |prefix: &str| {
            dimensions
                .iter()
                .find(|dimension| dimension.text.starts_with(prefix))
                .unwrap()
        };
        let ze = by("zE");
        assert_eq!(ze.text, format!("zE = {:.1}", p.z_e));
        assert_eq!((ze.from[0], ze.to[0]), (0.0, p.z_e));
        let b2 = p.hub.points[4][0] - p.z_e;
        assert_eq!(by("b2").text, format!("b2 = {b2:.1}"));
        // Cada cota a su altura: los textos no se pisan.
        assert_ne!(ze.from[1], by("b2").from[1]);
        let b1 = by("b1");
        assert_eq!(b1.text, format!("b1 = {:.1}", p.inlet_width));
        // Por fuera del círculo de la entrada.
        assert!(b1.from[0] < -p.inlet_width / 2.0);
        // Ninguna baja al eje: el dibujo conserva su escala.
        assert!(dimensions.iter().all(|d| d.from[1] > 0.0 && d.to[1] > 0.0));
        assert_eq!(dimensions.len(), 3);
    }
}
