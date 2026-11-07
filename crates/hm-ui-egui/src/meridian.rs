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

/// Color de cada curva del corte, los de CFturbo: tapa delantera en verde,
/// cubo en azul y los bordes en negro y gris oscuro. Una curva sin color
/// asignado se dibuja igual, en gris: no desaparece.
fn meridian_color(name: &str) -> Color32 {
    match name {
        "Tapa delantera" => Color32::from_rgb(40, 130, 30),
        "Tapa trasera" => Color32::from_rgb(25, 25, 235),
        "Borde de ataque" => Color32::from_rgb(20, 20, 20),
        "Borde de salida" => Color32::from_rgb(70, 70, 70),
        _ => Color32::GRAY,
    }
}

/// Grosor de cada curva del corte: las tapas más gruesas que los bordes.
fn meridian_width(name: &str) -> f32 {
    match name {
        "Tapa delantera" | "Tapa trasera" => 2.6,
        _ => 2.0,
    }
}

/// El papel del dibujo y su rejilla, muy tenue como en CFturbo.
const PAPER: Color32 = Color32::WHITE;
const GRID: Color32 = Color32::from_gray(232);
/// El rojo de CFturbo para círculos extremos, línea de centros y marcas.
const RED: Color32 = Color32::from_rgb(225, 55, 55);
/// El verde azulado de los extremos del borde de ataque.
const TEAL: Color32 = Color32::from_rgb(0, 125, 135);

/// Distancia en píxeles a la que un clic agarra un punto de control.
const GRAB_PIXELS: f32 = 12.0;

/// Lo que se dibuja de las cotas: segmentos y textos `(posición, texto, ancla)`.
#[derive(Debug, Default, PartialEq)]
struct Dimensioning {
    /// Líneas de referencia nombradas (dS, dH, d2), más tenues.
    references: Vec<Vec<[f64; 2]>>,
    /// Líneas de extensión y de cota con sus flechas.
    lines: Vec<Vec<[f64; 2]>>,
    /// Puntas de flecha: triángulos cerrados, para rellenarlos.
    arrows: Vec<[[f64; 2]; 3]>,
    texts: Vec<([f64; 2], String, egui::Align2)>,
}

/// El triángulo de una flecha cerrada con la punta en `tip`, hacia `direction`
/// (vector unitario), de largo `length`.
fn arrow_head(tip: [f64; 2], direction: [f64; 2], length: f64) -> [[f64; 2]; 3] {
    let normal = [-direction[1], direction[0]];
    let base = [
        tip[0] - direction[0] * length,
        tip[1] - direction[1] * length,
    ];
    let half = length * 0.35;
    [
        [base[0] + normal[0] * half, base[1] + normal[1] * half],
        tip,
        [base[0] - normal[0] * half, base[1] - normal[1] * half],
    ]
}

/// Cotas al estilo de CFturbo (todas en mm). Las líneas de referencia llevan el
/// nombre del diámetro: dS la de la tapa delantera a la entrada, dH la del cubo
/// y d2 la de la salida, más el eje. Del punto seleccionado `at`, su z desde
/// el borde de ataque, con la línea de cota bajo el eje, y su r desde el eje,
/// con la línea de cota a la derecha de todo el dibujo; cada una con líneas de
/// extensión, una flecha en el punto y el valor.
fn dimensioning(model: &MeridionalModel, at: [f64; 2]) -> Dimensioning {
    let p = &model.params;
    let (r1, r2) = (p.shroud.points[0][1], p.shroud.points[4][1]);
    let ri = p.d1i / 2.0;
    let z_max = p.hub.points[4][0];
    let gap = 0.08 * r2;
    // El círculo de la entrada sobresale hacia atrás su radio: los rótulos, fuera.
    let pad = p.inlet_width / 2.0 + 0.05 * r2;
    let mut out = Dimensioning::default();
    let span = |r: f64| vec![[-pad, r], [z_max + 3.0 * gap, r]];
    // Con dH = 0 el cubo va sobre el eje y las dos referencias coinciden: un solo rótulo.
    let hub_on_axis = ri < 1e-6;
    let mut named = vec![
        (
            0.0,
            if hub_on_axis {
                "eje, dH = 0.0".to_string()
            } else {
                "eje".to_string()
            },
        ),
        (r1, format!("dS = {:.1}", 2.0 * r1)),
        (r2, format!("d2 = {:.1}", 2.0 * r2)),
    ];
    if !hub_on_axis {
        named.push((ri, format!("dH = {:.1}", 2.0 * ri)));
    }
    for (r, name) in named {
        out.references.push(span(r));
        out.texts
            .push(([-pad, r], name, egui::Align2::RIGHT_BOTTOM));
    }
    // z: línea de cota bajo el eje.
    let y = -gap;
    let toward = |value: f64| if value >= 0.0 { 1.0 } else { -1.0 };
    out.lines.push(vec![[0.0, 0.0], [0.0, y - 0.3 * gap]]);
    out.lines.push(vec![[at[0], at[1]], [at[0], y - 0.3 * gap]]);
    out.lines.push(vec![[0.0, y], [at[0], y]]);
    out.arrows
        .push(arrow_head([at[0], y], [toward(at[0]), 0.0], 0.6 * gap));
    out.texts.push((
        [at[0] / 2.0, y],
        format!("{:.3}", at[0]),
        egui::Align2::CENTER_BOTTOM,
    ));
    // r: línea de cota a la derecha de todo el dibujo.
    let x = z_max + 2.0 * gap;
    out.lines.push(vec![[at[0], at[1]], [x + 0.3 * gap, at[1]]]);
    out.lines.push(vec![[x, 0.0], [x, at[1]]]);
    out.arrows
        .push(arrow_head([x, at[1]], [0.0, toward(at[1])], 0.6 * gap));
    out.texts.push((
        [x + 0.4 * gap, at[1] / 2.0],
        format!("{:.3}", at[1]),
        egui::Align2::LEFT_CENTER,
    ));
    out
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
    let fit_id = egui::Id::unique("meridian_fitted");
    ui.horizontal(|ui| {
        ui.label(RichText::new(summary(&model, &edits)).small());
        if ui.small_button("Encuadrar").clicked() {
            ui.data_mut(|data| data.remove::<bool>(fit_id));
        }
    });
    egui::CollapsingHeader::new(RichText::new("Datos del corte").small())
        .default_open(false)
        .show(ui, |ui| {
            ui.label(RichText::new(header(&model, &edits)).small().weak());
        });
    let engine = (edits != MeridianEdits::default())
        .then_some(auto.ok())
        .flatten();
    // Las gráficas del canal se pliegan para darle todo el alto al perfil.
    let graphs_id = ui.make_persistent_id("meridian_channel_graphs");
    let graphs = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        graphs_id,
        // Solo para las capturas: `HMF_GRAPHS=1` las abre.
        std::env::var_os("HMF_GRAPHS").is_some(),
    );
    let graphs_height = if graphs.is_open() {
        (ui.available_height() * 0.35).max(140.0)
    } else {
        32.0
    };
    ui.scope(|ui| {
        ui.visuals_mut().extreme_bg_color = PAPER;
        Plot::new("meridian")
            .data_aspect(1.0)
            .grid_color(GRID)
            .allow_drag(false)
            .height(ui.available_height() - graphs_height)
            .show(ui, |plot_ui| {
                // Al abrir (o con «Encuadrar») el corte llena el lienzo.
                let fitted = plot_ui.ctx().data(|data| data.get_temp::<bool>(fit_id));
                if fitted != Some(true)
                    && let Some((min, max)) = section_bounds(&model)
                {
                    plot_ui.set_plot_bounds(egui_plot::PlotBounds::from_min_max(min, max));
                    plot_ui.ctx().data_mut(|data| data.insert_temp(fit_id, true));
                }
                changed |= draw_section(plot_ui, &model, engine.as_ref(), boxes);
            });
    });
    graphs
        .show_header(ui, |ui| {
            ui.label(RichText::new("Gráficas del canal: A frente a M/Mmax y cm").small());
        })
        .body(|ui| channel_graphs(ui, &model, outcome.result.qth.si));
    changed
}

/// Rectángulo de las dos tapas con un 10 % de margen, para encuadrar el corte.
fn section_bounds(model: &MeridionalModel) -> Option<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for wall in Wall::ALL {
        for point in &model.curve(wall.name())?.points {
            for axis in 0..2 {
                min[axis] = min[axis].min(point[axis]);
                max[axis] = max[axis].max(point[axis]);
            }
        }
    }
    if !(min[0].is_finite() && max[0] > min[0] && max[1] > min[1]) {
        return None;
    }
    let margin = [(max[0] - min[0]) * 0.1, (max[1] - min[1]) * 0.1];
    Some((
        [min[0] - margin[0], min[1] - margin[1]],
        [max[0] + margin[0], max[1] + margin[1]],
    ))
}

/// Puntos por tapa de la gráfica de curvatura.
const CURVATURE_SAMPLES: usize = 200;

/// Una línea de una sola frase con lo esencial del corte.
fn summary(model: &MeridionalModel, edits: &MeridianEdits) -> String {
    let p = &model.params;
    format!(
        "zE {:.1} · dH {:.1} · radio mínimo tapa {:.1}, cubo {:.1} · borde {:.1}° · área de entrada {:.0} mm².{}",
        p.z_e,
        p.d1i,
        p.shroud.min_radius,
        p.hub.min_radius,
        p.edge_angle,
        p.edge_area,
        edited_note(edits)
    )
}

/// Dibuja el corte con el aspecto de CFturbo y deja agarrar y arrastrar los
/// puntos. Devuelve si cambió algo.
fn draw_section(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    engine: Option<&MeridionalModel>,
    boxes: &mut Meridian,
) -> bool {
    // La propuesta del motor, debajo de todo.
    if let Some(engine) = engine {
        for wall in Wall::ALL {
            if let Some(curve) = engine.curve(wall.name()) {
                plot_ui.line(
                    line("Propuesta del motor", curve.points.clone(), RED)
                        .width(1.0)
                        .style(egui_plot::LineStyle::dashed_loose()),
                );
            }
        }
    }
    // Las referencias dS, dH, d2 y el eje, gruesas y claras, y las cotas.
    let selected = selected_control(plot_ui.ctx());
    let at = model.control_position(selected).unwrap_or([0.0, 0.0]);
    let dimensioning = dimensioning(model, at);
    let grey = Color32::from_gray(110);
    for reference in dimensioning.references {
        plot_ui.line(line("Referencias", reference, Color32::from_gray(205)).width(3.5));
    }
    // Los círculos de área: la familia en gris claro, el de la entrada y el de
    // la salida en rojo, y la línea de sus centros.
    let circles = area_circles(model, AREA_CIRCLES);
    for (index, (centre, radius)) in circles.iter().enumerate() {
        let end = index == 0 || index + 1 == circles.len();
        plot_ui.line(
            line(
                "Círculos de área",
                circle_points(*centre, *radius),
                if end { RED } else { Color32::from_gray(185) },
            )
            .width(if end { 1.4 } else { 0.6 }),
        );
    }
    plot_ui.line(
        line(
            "Línea de centros",
            circles.iter().map(|(centre, _)| *centre).collect(),
            RED,
        )
        .width(1.0)
        .style(egui_plot::LineStyle::dashed_dense()),
    );
    // El polígono de control de cada tapa, P0 a P4, con el color de su tapa.
    for wall in Wall::ALL {
        plot_ui.line(
            line(
                "Polígono de control",
                model.params.wall(wall).points.to_vec(),
                meridian_color(wall.name()).gamma_multiply(0.55),
            )
            .width(1.0)
            .style(egui_plot::LineStyle::dashed_dense()),
        );
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
                Color32::from_gray(150),
            )
            .width(0.8)
            .style(egui_plot::LineStyle::dotted_dense()),
        );
    }
    // Las curvas del corte, por encima.
    for curve in &model.curves {
        plot_ui.line(
            line(
                &curve.name,
                curve.points.clone(),
                meridian_color(&curve.name),
            )
            .width(meridian_width(&curve.name)),
        );
    }
    // Las cotas del punto seleccionado.
    for arrow in dimensioning.arrows {
        plot_ui.polygon(
            egui_plot::Polygon::new("Cotas", arrow.to_vec())
                .fill_color(grey)
                .stroke(egui::Stroke::new(0.8, grey)),
        );
    }
    for dimension_line in dimensioning.lines {
        plot_ui.line(line("Cotas", dimension_line, grey).width(0.8));
    }
    for (position, text, anchor) in dimensioning.texts {
        plot_ui.text(
            egui_plot::Text::new(
                "Cotas",
                PlotPoint::new(position[0], position[1]),
                RichText::new(text).small(),
            )
            .anchor(anchor)
            .color(grey),
        );
    }
    // Los puntos de control: extremos P0 y P4 en cuadrados grises con borde,
    // los interiores y los del borde de ataque en círculos con borde.
    for wall in Wall::ALL {
        let colour = meridian_color(wall.name());
        let ends = model.params.wall(wall).points;
        let squares = vec![ends[0], ends[4]];
        plot_ui.points(
            Points::new("Extremos", squares.clone())
                .shape(egui_plot::MarkerShape::Square)
                .radius(5.5)
                .color(Color32::from_gray(40)),
        );
        plot_ui.points(
            Points::new("Extremos", squares)
                .shape(egui_plot::MarkerShape::Square)
                .radius(3.8)
                .color(Color32::from_gray(150)),
        );
        let inner: Vec<[f64; 2]> = model
            .handles()
            .iter()
            .filter(|handle| handle.control.wall == wall)
            .map(|handle| handle.at)
            .collect();
        plot_ui.points(
            Points::new("Puntos de control", inner.clone())
                .radius(5.5)
                .color(colour.gamma_multiply(0.4))
                .filled(false),
        );
        plot_ui.points(
            Points::new("Puntos de control", inner)
                .radius(4.2)
                .color(colour.gamma_multiply(0.55)),
        );
    }
    let edge: Vec<[f64; 2]> = model.edge_handles().iter().map(|h| h.at).collect();
    plot_ui.points(Points::new("Borde de ataque", edge).radius(4.5).color(TEAL));
    // El punto seleccionado, con un aro.
    plot_ui.points(
        Points::new("Seleccionado", vec![at])
            .radius(8.0)
            .color(Color32::from_gray(30))
            .filled(false),
    );
    // La curvatura máxima de cada tapa: donde está su radio mínimo.
    for wall in Wall::ALL {
        let shape = model.params.wall(wall);
        plot_ui.points(
            Points::new("Curvatura máxima", vec![shape.max_curvature_at])
                .radius(7.0)
                .shape(egui_plot::MarkerShape::Diamond)
                .color(RED)
                .filled(false),
        );
    }
    drag_handles(plot_ui, model, boxes)
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
    // El área de entrada del 1D, π·d1·b1 = π·(r1² − (d1i/2)²) con el d1i del 1D.
    let r1 = p.shroud.points[0][1];
    let one_d_area = std::f64::consts::PI * (r1 * r1 - (p.auto_d1i / 2.0).powi(2));
    format!(
        "Corte meridional, mm: z con el flujo (0 en el plano de entrada), r radial. zE = {:.1}, \
         dH = {:.1}, anchura de entrada del 1D = {:.1}. Borde de ataque: {:.1}° con la radial, longitud {:.1}, \
         área {:.0} mm² (1D: {:.0}). Radio mínimo: tapa delantera {shroud:.1}{verdict}; \
         trasera {:.1}. Cada tapa es una Bézier de 5 puntos: P2 se mueve dentro del rectángulo de los \
         extremos y P1 y P3 salen de ellos con su ángulo (° desde +z hacia +r) y su longitud. Gülich §7.2.2.1.{} El álabe sigue siendo 2D hasta F4. Huella {}.",
        p.z_e,
        p.d1i,
        p.inlet_width,
        p.edge_angle,
        p.edge_length,
        p.edge_area,
        one_d_area,
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
        ui.label(RichText::new("Entrada").strong());
        let auto_hub = auto.map(|model| length_text(model.params.auto_d1i));
        let fields: [(&str, &mut String, bool, String, &str); 3] = [
            (
                "dH",
                &mut boxes.hub_diameter,
                parsed.hub_diameter.is_none(),
                auto_hub.map_or_else(String::new, |text| format!("{text} (auto)")),
                "Diámetro del cubo en el plano de entrada, con unidad: de 0 (el cubo arranca en el \
                 eje) a menos que d1. Vacío: el que da el área del 1D con el borde radial.",
            ),
            (
                "Borde en tapa",
                &mut boxes.edge_shroud,
                parsed.edge_shroud.is_none(),
                "0 (auto)".to_string(),
                "Dónde se apoya el borde de ataque sobre la tapa delantera: fracción de su longitud \
                 desde el plano de entrada, de 0 a 0,9. La curva sigue aguas arriba del borde.",
            ),
            (
                "Borde en cubo",
                &mut boxes.edge_hub,
                parsed.edge_hub.is_none(),
                "0 (auto)".to_string(),
                "Dónde se apoya el borde de ataque sobre el cubo: fracción de su longitud desde el \
                 plano de entrada, de 0 a 0,9.",
            ),
        ];
        for (label, text, unread, hint, help) in fields {
            let bad = unread && !text.trim().is_empty();
            ui.label(RichText::new(label).small());
            let mut field = egui::TextEdit::singleline(text)
                .id_salt(("entry", label))
                .hint_text(hint)
                .desired_width(100.0);
            if bad {
                field = field.text_color(ui.visuals().error_fg_color);
            }
            changed |= ui.add(field).on_hover_text(help).changed();
        }
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
    if response.drag_started() || response.clicked() {
        let screen = plot_ui.screen_from_plot(pointer);
        let handles: Vec<(ControlPoint, [f32; 2])> = model
            .handles()
            .iter()
            .chain(model.edge_handles().iter())
            .map(|handle| {
                let at = plot_ui.screen_from_plot(PlotPoint::new(handle.at[0], handle.at[1]));
                (handle.control, [at.x, at.y])
            })
            .collect();
        let grabbed = nearest(&handles, [screen.x, screen.y], GRAB_PIXELS);
        if let Some(control) = grabbed {
            plot_ui
                .ctx()
                .data_mut(|data| data.insert_temp(selected_id(), control));
        }
        if response.drag_started() {
            plot_ui.ctx().data_mut(|data| data.insert_temp(id, grabbed));
        }
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

fn selected_id() -> egui::Id {
    egui::Id::unique("meridian_selected_point")
}

/// El punto de control seleccionado: el último que se tocó; al principio, P2
/// de la tapa delantera.
fn selected_control(ctx: &egui::Context) -> ControlPoint {
    ctx.data(|data| data.get_temp(selected_id()))
        .unwrap_or(ControlPoint {
            wall: Wall::Shroud,
            index: 2,
        })
}

/// Escribe en las cajas lo que dio el arrastre. Devuelve si cambió algo.
fn write_moved(boxes: &mut Meridian, control: ControlPoint, moved: Moved) -> bool {
    if let Moved::Fraction(fraction) = moved {
        let target = match control.wall {
            Wall::Shroud => &mut boxes.edge_shroud,
            Wall::Hub => &mut boxes.edge_hub,
        };
        let text = format!("{fraction:.3}");
        let changed = *target != text;
        *target = text;
        return changed;
    }
    let texts = wall_mut(boxes, control.wall);
    let updates: Vec<(&mut String, String)> = match (moved, control.index) {
        (Moved::Fraction(_), _) => return false,
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
    if edits.hub_diameter.is_some() {
        names.push("dH".to_string());
    }
    if edits.edge_shroud.is_some() || edits.edge_hub.is_some() {
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
             rendimiento. A la derecha, la curvatura κ (1/mm) de cada tapa frente a su M/Mmax, con el rombo \
             rojo en su máximo.",
        )
        .small()
        .weak(),
    );
    ui.columns(3, |columns| {
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
        Plot::new("wall_curvature")
            .legend(Legend::default().position(egui_plot::Corner::RightTop))
            .show(&mut columns[2], |plot_ui| {
                for wall in Wall::ALL {
                    let Ok(profile) = model.params.wall(wall).curvature_profile(CURVATURE_SAMPLES)
                    else {
                        continue;
                    };
                    let colour = meridian_color(wall.name());
                    // La marca de curvatura máxima, la misma del corte.
                    let peak = profile
                        .iter()
                        .copied()
                        .max_by(|a, b| a[1].abs().total_cmp(&b[1].abs()));
                    plot_ui.line(line(wall.name(), profile, colour));
                    if let Some(peak) = peak {
                        plot_ui.points(
                            Points::new("Curvatura máxima", vec![peak])
                                .shape(egui_plot::MarkerShape::Diamond)
                                .radius(5.0)
                                .color(RED),
                        );
                    }
                }
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
    fn the_dimensioning_measures_the_selected_point_from_the_axes() {
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
        let p = &model.params;
        let at = [p.z_e * 0.8, p.shroud.points[4][1] * 0.5];
        let d = dimensioning(&model, at);
        let texts: Vec<String> = d.texts.iter().map(|t| t.1.clone()).collect();
        let has = |text: String| texts.contains(&text);
        // Las tres referencias con su diámetro y el eje.
        assert!(has(format!("d2 = {:.1}", 2.0 * p.shroud.points[4][1])));
        assert!(has(format!("dS = {:.1}", 2.0 * p.shroud.points[0][1])));
        assert!(has(format!("dH = {:.1}", p.d1i)));
        assert!(has("eje".to_string()));
        assert_eq!(d.references.len(), 4);
        // Los dos valores del punto, con tres decimales.
        assert!(has(format!("{:.3}", at[0])));
        assert!(has(format!("{:.3}", at[1])));
        // La cota de z va bajo el eje y la de r a la derecha de todo.
        let z_line = d
            .lines
            .iter()
            .find(|l| l.len() == 2 && l[0][0] == 0.0 && l[1][0] == at[0] && l[0][1] == l[1][1])
            .unwrap();
        assert!(z_line[0][1] < 0.0);
        let r_line = d
            .lines
            .iter()
            .find(|l| l.len() == 2 && l[0][1] == 0.0 && l[1][1] == at[1] && l[0][0] == l[1][0])
            .unwrap();
        assert!(r_line[0][0] > p.hub.points[4][0]);
        // Las flechas tienen la punta en el extremo de la cota.
        let tips: Vec<[f64; 2]> = d.arrows.iter().map(|triangle| triangle[1]).collect();
        assert_eq!(tips.len(), 2);
        assert!(tips.contains(&[at[0], z_line[0][1]]));
        assert!(tips.contains(&[r_line[0][0], at[1]]));
    }

    #[test]
    fn with_the_hub_on_the_axis_the_axis_and_dh_share_one_label() {
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
        let edits = MeridianEdits {
            hub_diameter: Some(0.0),
            ..MeridianEdits::default()
        };
        let model = meridional_model_with(&result, &edits).unwrap();
        let d = dimensioning(&model, [10.0, 20.0]);
        let texts: Vec<&str> = d.texts.iter().map(|t| t.1.as_str()).collect();
        assert!(texts.contains(&"eje, dH = 0.0"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.starts_with("dH")), "{texts:?}");
        assert_eq!(d.references.len(), 3);
    }

    #[test]
    fn dragging_a_leading_edge_end_writes_its_fraction_in_its_box() {
        let mut boxes = Meridian::default();
        let shroud = ControlPoint {
            wall: Wall::Shroud,
            index: 0,
        };
        let hub = ControlPoint {
            wall: Wall::Hub,
            index: 0,
        };
        assert!(write_moved(&mut boxes, shroud, Moved::Fraction(0.25)));
        assert!(write_moved(&mut boxes, hub, Moved::Fraction(0.5)));
        assert_eq!(
            (boxes.edge_shroud.as_str(), boxes.edge_hub.as_str()),
            ("0.250", "0.500")
        );
        // El mismo valor otra vez no cuenta como cambio.
        assert!(!write_moved(&mut boxes, shroud, Moved::Fraction(0.25)));
        // Lo escrito lo entiende el motor.
        let edits = hm_engine::meridian_edits_of(&boxes);
        assert_eq!((edits.edge_shroud, edits.edge_hub), (Some(0.25), Some(0.5)));
    }
}
