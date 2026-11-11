//! Gráfica β(r) del álabe: la ley de ángulo con sus manijas arrastrables.

use eframe::egui;
use egui_plot::{Plot, PlotPoint, Points};
use hm_design::blade_camber::BladeCamber;
use hm_design::blade_setup::{BladeFamily, BladeSetup, CamberBows};
use hm_design::geometry::{BLADE_BETA_MIN_DEG, beta_profile};
use hm_design::{BETA_BOW_RANGE_DEG, INCIDENCE_RANGE_DEG};

use crate::curves::line;
use crate::model::{Inputs, Outcome};

/// Fracción de r de cada manija interior.
const HANDLE_FRACTIONS: [f64; 2] = [1.0 / 3.0, 2.0 / 3.0];

/// Radio, en píxeles, dentro del cual se agarra una manija.
pub(super) const GRAB_PIXELS: f32 = 14.0;

/// La ley β(r) de un álabe: extremos y desviación (rad, m y grados).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BetaGraph {
    /// Radios de entrada y de salida, m.
    pub r: (f64, f64),
    /// β en la entrada y en la salida, rad.
    pub beta: (f64, f64),
    /// Desviación en las manijas de 1/3 y 2/3 de r, grados.
    pub bow: (f64, f64),
}

impl BetaGraph {
    /// La ley de una pared con la abscisa x (0 a 1) en lugar de r: así se
    /// reutiliza todo lo de arriba, y `r_mm(f)` vale `f`.
    fn on_x(beta: (f64, f64), bow: (f64, f64)) -> Self {
        Self {
            r: (0.0, 1.0e-3),
            beta,
            bow,
        }
    }

    fn r_mm(&self, fraction: f64) -> f64 {
        (self.r.0 + (self.r.1 - self.r.0) * fraction) * 1000.0
    }

    fn line_deg(&self, fraction: f64) -> f64 {
        (self.beta.0 + (self.beta.1 - self.beta.0) * fraction).to_degrees()
    }

    /// Lo mismo que comprueba el motor: con desviación, β queda entre el
    /// mínimo y 90°. Sin desviación no se comprueba nada, como en el motor.
    fn is_valid(&self) -> bool {
        if self.bow == (0.0, 0.0) {
            return true;
        }
        let (min, max, _) = hm_design::beta_profile_range(self.beta.0, self.beta.1, self.bow);
        min >= BLADE_BETA_MIN_DEG.to_radians() && max < std::f64::consts::FRAC_PI_2
    }

    /// La misma gráfica con β1 (grados) cambiado.
    fn with_beta1_deg(&self, deg: f64) -> Self {
        Self {
            beta: (deg.to_radians(), self.beta.1),
            ..*self
        }
    }

    /// La misma gráfica con β2 (grados) cambiado.
    fn with_beta2_deg(&self, deg: f64) -> Self {
        Self {
            beta: (self.beta.0, deg.to_radians()),
            ..*self
        }
    }

    /// La curva: `[r en mm, β en grados]`.
    pub fn curve(&self, samples: usize) -> Vec<[f64; 2]> {
        let samples = samples.max(1);
        (0..=samples)
            .map(|index| {
                let fraction = index as f64 / samples as f64;
                [
                    self.r_mm(fraction),
                    beta_profile(self.beta.0, self.beta.1, self.bow, fraction).to_degrees(),
                ]
            })
            .collect()
    }

    /// La recta de β1 a β2 que da el 1D, en gris.
    pub fn straight(&self) -> [[f64; 2]; 2] {
        [
            [self.r_mm(0.0), self.line_deg(0.0)],
            [self.r_mm(1.0), self.line_deg(1.0)],
        ]
    }

    /// Las dos manijas interiores: los puntos de control de la desviación.
    pub fn handles(&self) -> [[f64; 2]; 2] {
        let at = |index: usize, bow: f64| {
            let fraction = HANDLE_FRACTIONS[index];
            [self.r_mm(fraction), self.line_deg(fraction) + bow]
        };
        [at(0, self.bow.0), at(1, self.bow.1)]
    }

    /// Los dos extremos: β1 (entrada) y β2 (salida).
    pub fn ends(&self) -> [[f64; 2]; 2] {
        self.straight()
    }

    /// La desviación, en grados, de arrastrar la manija `index` a `beta_deg`.
    pub fn bow_from_handle(&self, index: usize, beta_deg: f64) -> f64 {
        let index = index.min(1);
        let wanted = beta_deg - self.line_deg(HANDLE_FRACTIONS[index]);
        // Con los dos decimales de la caja, para que el motor vea lo mismo.
        let mut bow = wanted.clamp(BETA_BOW_RANGE_DEG.0, BETA_BOW_RANGE_DEG.1);
        bow = (bow * 100.0).round() / 100.0;
        // β no sale del rango del motor: se acerca la manija a la recta hasta
        // que cumpla.
        let with = |value: f64| Self {
            bow: if index == 0 {
                (value, self.bow.1)
            } else {
                (self.bow.0, value)
            },
            ..*self
        };
        while bow != 0.0 && !with(bow).is_valid() {
            bow -= bow.signum() * 0.25;
            if bow.abs() < 0.25 {
                bow = 0.0;
            }
        }
        bow
    }
}

/// Un valor que pudo quedar en el límite de su rango.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Clamped {
    pub value: f64,
    /// `true` si el valor pedido se salía del rango y se recortó.
    pub limited: bool,
}

/// Incidencia que da β1 = `target_deg`. β1 = β1B sin incidencia + incidencia,
/// y la incidencia solo vale de 0 al máximo de `INCIDENCE_RANGE_DEG`.
pub(super) fn incidence_for_beta1(
    target_deg: f64,
    beta1b_deg: f64,
    applied_incidence_deg: f64,
) -> Clamped {
    let base = beta1b_deg - applied_incidence_deg;
    let wanted = target_deg - base;
    let value = wanted.clamp(INCIDENCE_RANGE_DEG.0, INCIDENCE_RANGE_DEG.1);
    Clamped {
        value,
        limited: (wanted - value).abs() > 1.0e-9,
    }
}

/// β2 que se fija al arrastrar el extremo de salida, entre 1° y 89°.
pub(super) fn beta2_for_target(target_deg: f64) -> Clamped {
    let value = target_deg.clamp(1.0, 89.0);
    Clamped {
        value,
        limited: (target_deg - value).abs() > 1.0e-9,
    }
}

/// El texto de la caja β2.
pub(super) fn beta2_text(value_deg: f64) -> String {
    format!("{value_deg:.2} deg")
}

/// Lo que el paso edita: las cuatro cajas de la gráfica, como texto, y la
/// forma del álabe con su número de líneas de corriente.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct BetaSnapshot {
    pub bow1: String,
    pub bow2: String,
    pub incidence: String,
    pub beta2: String,
    pub blade: BladeSetup,
}

impl BetaSnapshot {
    pub fn of(inputs: &Inputs) -> Self {
        Self {
            bow1: inputs.beta_bow1.clone(),
            bow2: inputs.beta_bow2.clone(),
            incidence: inputs.incidence.clone(),
            beta2: inputs.beta2.clone(),
            blade: inputs.blade.clone(),
        }
    }

    pub fn apply(&self, inputs: &mut Inputs) {
        inputs.beta_bow1.clone_from(&self.bow1);
        inputs.beta_bow2.clone_from(&self.bow2);
        inputs.incidence.clone_from(&self.incidence);
        inputs.beta2.clone_from(&self.beta2);
        inputs.blade.clone_from(&self.blade);
    }
}

/// Pasos que se recuerdan para deshacer.
const HISTORY_STEPS: usize = 200;

/// Cambios seguidos a menos de esto (segundos) forman un solo paso: un
/// arrastre entero se deshace de una vez.
const HISTORY_GAP: f64 = 0.6;

/// Deshacer y rehacer de lo editado en la gráfica de β(r).
#[derive(Debug, Default)]
pub(super) struct BetaHistory {
    last: BetaSnapshot,
    undo: Vec<BetaSnapshot>,
    redo: Vec<BetaSnapshot>,
    last_change: f64,
}

impl BetaHistory {
    /// Las cajas cambiaron por otra vía (se escribió en una caja, se abrió
    /// un archivo): se olvida lo anterior.
    pub fn follow(&mut self, now: &BetaSnapshot) {
        if *now != self.last {
            self.last = now.clone();
            self.undo.clear();
            self.redo.clear();
        }
    }

    /// Anota un cambio hecho en la gráfica, en el instante `time`.
    pub fn record(&mut self, now: &BetaSnapshot, time: f64) {
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

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// El paso anterior, dado lo de ahora; `None` si no hay.
    pub fn undo(&mut self, current: &BetaSnapshot) -> Option<BetaSnapshot> {
        let previous = self.undo.pop()?;
        self.redo.push(current.clone());
        self.last = previous.clone();
        self.last_change = f64::NEG_INFINITY;
        Some(previous)
    }

    /// El paso deshecho, dado lo de ahora; `None` si no hay.
    pub fn redo(&mut self, current: &BetaSnapshot) -> Option<BetaSnapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        self.last = next.clone();
        self.last_change = f64::NEG_INFINITY;
        Some(next)
    }
}

/// Vacía las dos desviaciones y no toca los extremos.
pub(super) fn straighten(inputs: &mut Inputs) {
    inputs.beta_bow1.clear();
    inputs.beta_bow2.clear();
}

/// Pone a cero las cuatro manijas 3D (cubo y tapa). Las cajas del 2D, los
/// extremos y la forma no se tocan.
pub(super) fn straighten_3d(inputs: &mut Inputs) {
    inputs.blade.bows = CamberBows::default();
}

/// Qué pared lleva la manija.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Wall {
    Hub,
    Shroud,
}

impl Wall {
    fn name(self) -> &'static str {
        match self {
            Self::Hub => "Cubo",
            Self::Shroud => "Tapa",
        }
    }
}

/// Las leyes β(x) de un álabe 3D: una curva por línea de corriente, `[x, β°]`
/// con x la fracción de longitud meridional, y las manijas de cubo y tapa. La
/// ley de cada pared es la misma de [`BetaGraph`], con x en lugar de la
/// fracción de r.
pub(super) struct BetaGraph3d {
    curves: Vec<Vec<[f64; 2]>>,
    hub: BetaGraph,
    shroud: BetaGraph,
    /// ψ, β de entrada y β de salida (rad) de cada línea.
    laws: Vec<(f64, f64, f64)>,
    bows: CamberBows,
    /// Solo la libre integra las intermedias con su ley.
    checks_inner_lines: bool,
}

impl BetaGraph3d {
    pub fn of(outcome: &Outcome, camber: &BladeCamber) -> Self {
        let bows = outcome.inputs.blade.bows;
        let ends = |line: &hm_design::blade_camber::CamberLine| {
            (
                line.points.first().map_or(0.0, |point| point.beta),
                line.points.last().map_or(0.0, |point| point.beta),
            )
        };
        let wall = |line: Option<&hm_design::blade_camber::CamberLine>, bow| {
            BetaGraph::on_x(line.map_or((0.0, 0.0), ends), bow)
        };
        Self {
            curves: camber
                .lines
                .iter()
                .map(|line| {
                    line.points
                        .iter()
                        .map(|point| [point.x, point.beta.to_degrees()])
                        .collect()
                })
                .collect(),
            hub: wall(camber.lines.first(), bows.hub),
            shroud: wall(camber.lines.last(), bows.shroud),
            laws: camber
                .lines
                .iter()
                .map(|line| {
                    let (first, last) = ends(line);
                    (line.psi, first, last)
                })
                .collect(),
            bows,
            checks_inner_lines: camber.shape == hm_design::blade_setup::BladeShape::Free3D,
        }
    }

    /// Los dos extremos arrastrables: la entrada de la tapa (incidencia) y la
    /// salida (β2, la misma en todas las líneas).
    fn ends(&self) -> [[f64; 2]; 2] {
        [
            [0.0, self.shroud.beta.0.to_degrees()],
            [1.0, self.hub.beta.1.to_degrees()],
        ]
    }

    /// Una curva por línea, del cubo a la tapa.
    pub fn curves(&self) -> Vec<Vec<[f64; 2]>> {
        self.curves.clone()
    }

    fn wall(&self, wall: Wall) -> &BetaGraph {
        match wall {
            Wall::Hub => &self.hub,
            Wall::Shroud => &self.shroud,
        }
    }

    /// Las dos manijas de la pared, en x = 1/3 y 2/3: la recta de la pared más
    /// la desviación (los puntos de control de la Bézier).
    pub fn handles(&self, wall: Wall) -> [[f64; 2]; 2] {
        self.wall(wall).handles()
    }

    /// Lo mismo que comprueba el motor: con desviación, β de las paredes (y de
    /// las intermedias de la libre) queda entre el mínimo y 90°. `shift` mueve
    /// β de entrada de todas las líneas (la incidencia) y `last` fija β2.
    fn is_valid_with(&self, bows: &CamberBows, shift: f64, last: Option<f64>) -> bool {
        let law = |first: f64, end: f64, bow| {
            BetaGraph::on_x((first + shift, last.unwrap_or(end)), bow).is_valid()
        };
        if !law(self.hub.beta.0, self.hub.beta.1, bows.hub)
            || !law(self.shroud.beta.0, self.shroud.beta.1, bows.shroud)
        {
            return false;
        }
        !self.checks_inner_lines
            || self
                .laws
                .iter()
                .all(|&(psi, first, end)| law(first, end, bows.at(psi)))
    }

    /// Las manijas con la `index` de `wall` llevada a `beta_deg`, recortada al
    /// rango del motor y a dos decimales; `None` si con ellas β saldría de 5°
    /// a 90° (el arrastre se queda en el último valor válido).
    pub fn with_handle(&self, wall: Wall, index: usize, beta_deg: f64) -> Option<CamberBows> {
        let index = index.min(1);
        let wanted = beta_deg - self.wall(wall).line_deg(HANDLE_FRACTIONS[index]);
        let bow = wanted.clamp(BETA_BOW_RANGE_DEG.0, BETA_BOW_RANGE_DEG.1);
        let bow = (bow * 100.0).round() / 100.0;
        let mut bows = self.bows;
        let pair = match wall {
            Wall::Hub => &mut bows.hub,
            Wall::Shroud => &mut bows.shroud,
        };
        if index == 0 {
            pair.0 = bow;
        } else {
            pair.1 = bow;
        }
        self.is_valid_with(&bows, 0.0, None).then_some(bows)
    }
}

/// La línea de lectura: β1, β2, envolvimiento y carga. El segundo valor es
/// `true` si la carga pasa de la admisible más el margen de Gülich.
pub(super) fn reading_text(
    beta1_deg: f64,
    beta2_deg: f64,
    wrap_deg: f64,
    loading: f64,
    allowable: f64,
    cylindrical: bool,
) -> (String, bool) {
    // Con nq por encima del límite el álabe cilíndrico es solo una
    // simplificación y el motor no juzga la carga: aquí tampoco.
    let flagged = cylindrical && loading > (1.0 + hm_design::BLADE_LOADING_MARGIN) * allowable;
    (
        format!(
            "β1 = {beta1_deg:.1}° · β2 = {beta2_deg:.1}° · envolvimiento = {wrap_deg:.0}° · \
             carga ξeff = {loading:.2} (admisible {allowable:.2})"
        ),
        flagged,
    )
}

/// La gráfica de β(r) de este diseño, si se puede construir.
fn graph_of(outcome: &Outcome) -> BetaGraph {
    let result = &outcome.result;
    BetaGraph {
        r: (result.d1.si / 2.0, result.d2.si / 2.0),
        beta: (result.beta1b, result.selection.beta2_deg.to_radians()),
        bow: result.selection.blade_bow_deg,
    }
}

fn bow_text(value: f64) -> String {
    format!("{value:.2} deg")
}

/// Dibuja la gráfica y deja arrastrar las manijas interiores. Devuelve
/// `true` si se escribió una caja de `inputs`, y hay que recalcular.
pub(super) fn blade_beta_view(
    ui: &mut egui::Ui,
    outcome: &Outcome,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
) -> bool {
    use egui::{Key, KeyboardShortcut, Modifiers};
    history.follow(&BetaSnapshot::of(inputs));
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
    let mut edited = false;
    if crate::blade_inlet::blade_setup_controls(ui, &mut inputs.blade) {
        history.record(&BetaSnapshot::of(inputs), ui.input(|input| input.time));
        edited = true;
    }
    let camber = crate::blade_inlet::camber_of(outcome);
    let three_d = inputs.blade.shape.family() == BladeFamily::ThreeD;
    ui.horizontal(|ui| {
        let undo = ui.add_enabled(history.can_undo(), egui::Button::new("Deshacer"));
        let redo = ui.add_enabled(history.can_redo(), egui::Button::new("Rehacer"));
        let has_bow = if three_d {
            inputs.blade.bows != CamberBows::default()
        } else {
            !inputs.beta_bow1.trim().is_empty() || !inputs.beta_bow2.trim().is_empty()
        };
        let flat = ui
            .add_enabled(has_bow, egui::Button::new("Volver a recta"))
            .on_hover_text(if three_d {
                "Pone a cero las cuatro manijas de cubo y tapa; los extremos no se tocan."
            } else {
                "Vacía las dos desviaciones; los extremos no se tocan."
            });
        if undo.clicked() || undo_key {
            let current = BetaSnapshot::of(inputs);
            if let Some(previous) = history.undo(&current) {
                previous.apply(inputs);
                edited = true;
            }
        }
        if redo.clicked() || redo_keys {
            let current = BetaSnapshot::of(inputs);
            if let Some(next) = history.redo(&current) {
                next.apply(inputs);
                edited = true;
            }
        }
        if flat.clicked() {
            if three_d {
                straighten_3d(inputs);
            } else {
                straighten(inputs);
            }
            history.record(&BetaSnapshot::of(inputs), ui.input(|input| input.time));
            edited = true;
        }
    });
    match &camber {
        Ok(Some(camber)) => {
            let mut chart = crate::blade_development::BladeChart::stored(ui.ctx());
            ui.horizontal(|ui| {
                ui.selectable_value(&mut chart, crate::blade_development::BladeChart::Beta, "β");
                ui.selectable_value(
                    &mut chart,
                    crate::blade_development::BladeChart::Thickness,
                    "Espesor",
                );
                ui.selectable_value(
                    &mut chart,
                    crate::blade_development::BladeChart::Development,
                    "Desarrollo",
                );
            });
            chart.store(ui.ctx());
            match chart {
                crate::blade_development::BladeChart::Beta => {
                    edited |= beta_3d_plot(ui, outcome, inputs, history, camber);
                }
                crate::blade_development::BladeChart::Thickness => {
                    edited |= crate::blade_thickness::thickness_view(
                        ui, outcome, inputs, history, camber,
                    );
                }
                crate::blade_development::BladeChart::Development => {
                    crate::blade_development::development_view(ui, camber);
                }
            }
            edited |= bow_fields(ui, inputs, history);
        }
        Err(message) => {
            ui.label(format!("Sin línea media 3D: {message}."));
            edited |= bow_fields(ui, inputs, history);
        }
        Ok(None) => {
            // Mismo selector que en 3D, con «β» elegida: el espesor y el
            // perfil de las formas 2D llegan en la etapa 3 del espesor.
            ui.horizontal(|ui| {
                let _ = ui.selectable_label(true, "β");
                ui.add_enabled_ui(false, |ui| {
                    let _ = ui.selectable_label(false, "Espesor");
                    let _ = ui.selectable_label(false, "Perfil");
                });
                ui.label(
                    egui::RichText::new(
                        "El espesor y el perfil de las formas 2D llegan en la etapa 3.",
                    )
                    .small()
                    .weak(),
                );
            });
            edited |= beta_2d_plot(ui, outcome, inputs, history);
        }
    }
    crate::blade_inlet::inlet_table(ui, outcome);
    edited
}

/// Las cuatro manijas 3D como números, en grados: se pueden corregir también
/// cuando la línea media falla y no hay gráfica.
fn bow_fields(ui: &mut egui::Ui, inputs: &mut Inputs, history: &mut BetaHistory) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let bows = &mut inputs.blade.bows;
        for (label, value) in [
            ("Cubo 1/3", &mut bows.hub.0),
            ("Cubo 2/3", &mut bows.hub.1),
            ("Tapa 1/3", &mut bows.shroud.0),
            ("Tapa 2/3", &mut bows.shroud.1),
        ] {
            ui.label(label);
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(BETA_BOW_RANGE_DEG.0..=BETA_BOW_RANGE_DEG.1)
                        .speed(0.1)
                        .fixed_decimals(2)
                        .suffix("°"),
                )
                .changed();
        }
    });
    if changed {
        history.record(&BetaSnapshot::of(inputs), ui.input(|input| input.time));
    }
    changed
}

/// La gráfica β(x) de un álabe 3D: una curva por línea, cubo y tapa gruesas con
/// sus manijas. Devuelve `true` si se escribió una caja.
fn beta_3d_plot(
    ui: &mut egui::Ui,
    outcome: &Outcome,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
    camber: &BladeCamber,
) -> bool {
    let graph = BetaGraph3d::of(outcome, camber);
    ui.label(
        egui::RichText::new(
            "β(x) de cada línea de corriente, en grados; x es la fracción de la longitud \
             meridional del álabe. Gruesas: cubo y tapa; sus manijas naranjas dan la forma. \
             Azules: la entrada de la tapa mueve la incidencia y la salida fija β2.",
        )
        .small()
        .weak(),
    );
    let status_id = egui::Id::unique("blade_beta_status_3d");
    let palette = crate::theme::palette(ui.ctx());
    let mut edited = false;
    Plot::new("blade_beta_3d")
        .height(190.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .x_axis_label("x, fracción de longitud meridional")
        .y_axis_label("β, °")
        .show(ui, |plot_ui| {
            let curves = graph.curves();
            let last = curves.len().saturating_sub(1);
            for (index, curve) in curves.into_iter().enumerate() {
                let (name, color, width) = match index {
                    0 => ("Cubo", palette.hub, 2.5),
                    i if i == last => ("Tapa", palette.shroud, 2.5),
                    _ => ("Intermedias", palette.construction, 1.0),
                };
                plot_ui.line(line(name, curve, color).width(width));
            }
            for wall in [Wall::Hub, Wall::Shroud] {
                let law = graph.wall(wall);
                let [a, b] = law.straight();
                let handles = graph.handles(wall);
                plot_ui.line(
                    line("", vec![a, handles[0], handles[1], b], palette.copper)
                        .width(1.0)
                        .style(egui_plot::LineStyle::dashed_loose()),
                );
            }
            let handles: Vec<[f64; 2]> = [Wall::Hub, Wall::Shroud]
                .into_iter()
                .flat_map(|wall| graph.handles(wall))
                .collect();
            plot_ui.points(
                Points::new("Manijas", handles)
                    .color(palette.copper)
                    .radius(6.0),
            );
            plot_ui.points(
                Points::new("Extremos", graph.ends().to_vec())
                    .color(palette.blade)
                    .radius(6.0),
            );
            if let Some(drag) = drag_handles_3d(plot_ui, &graph, inputs) {
                plot_ui
                    .ctx()
                    .data_mut(|data| data.insert_temp(status_id, drag.status));
                edited |= drag.written;
            }
        });
    if edited {
        history.record(&BetaSnapshot::of(inputs), ui.input(|input| input.time));
    }
    if let Some(status) = ui.ctx().data(|data| data.get_temp::<String>(status_id)) {
        ui.label(egui::RichText::new(status).small());
    }
    let beta = |line: Option<&hm_design::blade_camber::CamberLine>| {
        line.and_then(|line| line.points.first())
            .map_or(f64::NAN, |point| point.beta.to_degrees())
    };
    let wraps = camber.lines.iter().map(|line| line.wrap.to_degrees());
    let low = wraps.clone().fold(f64::INFINITY, f64::min);
    let high = wraps.fold(f64::NEG_INFINITY, f64::max);
    ui.label(
        egui::RichText::new(format!(
            "β1B = {:.1}° (cubo) a {:.1}° (tapa) · β2 = {:.1}° · envolvimiento de {low:.0} a \
             {high:.0}°",
            beta(camber.lines.first()),
            beta(camber.lines.last()),
            outcome.result.selection.beta2_deg,
        ))
        .small(),
    );
    edited
}

/// La gráfica de β(r) del álabe 2D, con sus tres manijas y extremos.
fn beta_2d_plot(
    ui: &mut egui::Ui,
    outcome: &Outcome,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
) -> bool {
    let graph = graph_of(outcome);
    let mut edited = false;
    ui.label(
        egui::RichText::new(
            "β(r) del álabe, en grados. Gris: la recta del 1D. Naranjas: dan la forma. \
             Azules: los extremos; β1 mueve la incidencia y β2 se fija a mano (vacía su \
             caja para volver a Euler).",
        )
        .small()
        .weak(),
    );
    let status_id = egui::Id::unique("blade_beta_status");
    let mut dragged = false;
    let palette = crate::theme::palette(ui.ctx());
    Plot::new("blade_beta")
        .height(190.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .x_axis_label("r, mm")
        .y_axis_label("β, °")
        .show(ui, |plot_ui| {
            let [a, b] = graph.straight();
            plot_ui.line(line("Recta del 1D", vec![a, b], palette.reference).width(1.0));
            plot_ui.line(line("β(r)", graph.curve(60), palette.blade));
            let handles = graph.handles();
            // Polígono de control, para que se vea qué tira de la curva.
            plot_ui.line(
                line("", vec![a, handles[0], handles[1], b], palette.copper)
                    .width(1.0)
                    .style(egui_plot::LineStyle::dashed_loose()),
            );
            plot_ui.points(
                Points::new("Manijas", handles.to_vec())
                    .color(palette.copper)
                    .radius(6.0),
            );
            plot_ui.points(
                Points::new("Extremos", graph.ends().to_vec())
                    .color(palette.blade)
                    .radius(6.0),
            );
            if let Some(drag) = drag_handles(plot_ui, &graph, inputs) {
                plot_ui
                    .ctx()
                    .data_mut(|data| data.insert_temp(status_id, drag.status));
                if drag.written {
                    edited = true;
                    dragged = true;
                }
            }
        });
    if dragged {
        history.record(&BetaSnapshot::of(inputs), ui.input(|input| input.time));
    }
    if let Some(status) = ui.ctx().data(|data| data.get_temp::<String>(status_id)) {
        ui.label(egui::RichText::new(status).small());
    }
    let result = &outcome.result;
    let wrap_deg = outcome
        .geometry
        .as_ref()
        .map_or(f64::NAN, |shape| shape.impeller.wrap_angle.to_degrees());
    let (text, flagged) = reading_text(
        graph.beta.0.to_degrees(),
        graph.beta.1.to_degrees(),
        wrap_deg,
        result.blade_loading,
        hm_design::blade_loading_allowable(result.nq),
        result.nq <= hm_design::CYLINDRICAL_BLADE_MAX_NQ,
    );
    let reading = egui::RichText::new(text).small();
    ui.label(if flagged {
        reading.color(crate::theme::palette(ui.ctx()).marker)
    } else {
        reading
    });
    if flagged {
        ui.label(
            egui::RichText::new(
                "La carga pasa de la admisible de Gülich más un 15 %: el álabe es corto \
                 para su número.",
            )
            .small()
            .color(crate::theme::palette(ui.ctx()).marker),
        );
    }
    edited
}

/// La incidencia que lleva la caja, o la de por defecto si está vacía.
pub(super) fn applied_incidence_deg(inputs: &Inputs) -> f64 {
    incidence_text_deg(&inputs.incidence)
}

/// La incidencia de una caja, en grados, con su unidad (`deg` o `rad`). Vacía
/// o ilegible, la de por defecto.
fn incidence_text_deg(text: &str) -> f64 {
    if text.trim().is_empty() {
        return hm_design::geometry::DEFAULT_INCIDENCE_DEG;
    }
    hm_units::parse_quantity(text.trim())
        .map_or(hm_design::geometry::DEFAULT_INCIDENCE_DEG, |quantity| {
            quantity.si.to_degrees()
        })
}

/// Agarra el punto más cercano al empezar a arrastrar y, mientras dura,
/// escribe en su caja lo que da el puntero: la desviación (manijas 0 y 1), la
/// incidencia (2, β1) o el β2 fijado (3). Devuelve lo que dice la lectura.
fn drag_handles(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    graph: &BetaGraph,
    inputs: &mut Inputs,
) -> Option<Drag> {
    let id = egui::Id::unique("blade_beta_dragged");
    let response = plot_ui.response().clone();
    let pointer = plot_ui.pointer_coordinate()?;
    if response.drag_started() {
        let at = plot_ui.screen_from_plot(pointer);
        let [interior_a, interior_b] = graph.handles();
        let [end_a, end_b] = graph.ends();
        let grabbed = [interior_a, interior_b, end_a, end_b]
            .iter()
            .enumerate()
            .map(|(index, handle)| {
                let screen = plot_ui.screen_from_plot(PlotPoint::new(handle[0], handle[1]));
                (index, screen.distance(at))
            })
            .filter(|(_, distance)| *distance <= GRAB_PIXELS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index);
        plot_ui.ctx().data_mut(|data| data.insert_temp(id, grabbed));
    }
    let grabbed: Option<usize> = plot_ui.ctx().data(|data| data.get_temp(id)).flatten();
    if response.drag_stopped() {
        plot_ui
            .ctx()
            .data_mut(|data| data.remove::<Option<usize>>(id));
    }
    let (Some(index), true) = (grabbed, response.dragged()) else {
        return None;
    };
    let (target, text, status) = match index {
        0 | 1 => {
            let bow = graph.bow_from_handle(index, pointer.y);
            let target = if index == 0 {
                &mut inputs.beta_bow1
            } else {
                &mut inputs.beta_bow2
            };
            (target, bow_text(bow), format!("Desviación = {bow:.1}°"))
        }
        2 => {
            let applied = applied_incidence_deg(inputs);
            let clamped = incidence_for_beta1(pointer.y, graph.beta.0.to_degrees(), applied);
            let wanted = graph.beta.0.to_degrees() - applied + clamped.value;
            if !graph.with_beta1_deg(wanted).is_valid() {
                return Some(Drag::refused());
            }
            let mut status = format!("Incidencia = {:.1}°", clamped.value);
            if clamped.limited {
                status.push_str(&format!(
                    " (límite: la incidencia va de {} a {}°)",
                    INCIDENCE_RANGE_DEG.0, INCIDENCE_RANGE_DEG.1
                ));
            }
            (&mut inputs.incidence, bow_text(clamped.value), status)
        }
        _ => {
            let clamped = beta2_for_target(pointer.y);
            if !graph.with_beta2_deg(clamped.value).is_valid() {
                return Some(Drag::refused());
            }
            let mut status = format!("β2 fijado = {:.1}°", clamped.value);
            if clamped.limited {
                status.push_str(" (límite: de 1 a 89°)");
            }
            (&mut inputs.beta2, beta2_text(clamped.value), status)
        }
    };
    let written = *target != text;
    *target = text;
    Some(Drag { status, written })
}

/// Como [`drag_handles`], para la gráfica 3D: seis puntos, las dos manijas del
/// cubo (0, 1) y de la tapa (2, 3), la entrada de la tapa (4, incidencia) y la
/// salida (5, β2). Un arrastre que sacaría β de 5° a 90° no escribe nada.
fn drag_handles_3d(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    graph: &BetaGraph3d,
    inputs: &mut Inputs,
) -> Option<Drag> {
    let id = egui::Id::unique("blade_beta_dragged_3d");
    let response = plot_ui.response().clone();
    let pointer = plot_ui.pointer_coordinate()?;
    let points: Vec<[f64; 2]> = {
        let [hub_a, hub_b] = graph.handles(Wall::Hub);
        let [shroud_a, shroud_b] = graph.handles(Wall::Shroud);
        let [inlet, outlet] = graph.ends();
        vec![hub_a, hub_b, shroud_a, shroud_b, inlet, outlet]
    };
    if response.drag_started() {
        let at = plot_ui.screen_from_plot(pointer);
        let grabbed = points
            .iter()
            .enumerate()
            .map(|(index, point)| {
                let screen = plot_ui.screen_from_plot(PlotPoint::new(point[0], point[1]));
                (index, screen.distance(at))
            })
            .filter(|(_, distance)| *distance <= GRAB_PIXELS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index);
        plot_ui.ctx().data_mut(|data| data.insert_temp(id, grabbed));
    }
    let grabbed: Option<usize> = plot_ui.ctx().data(|data| data.get_temp(id)).flatten();
    if response.drag_stopped() {
        plot_ui
            .ctx()
            .data_mut(|data| data.remove::<Option<usize>>(id));
    }
    let (Some(index), true) = (grabbed, response.dragged()) else {
        return None;
    };
    match index {
        0..=3 => {
            let wall = if index < 2 { Wall::Hub } else { Wall::Shroud };
            let Some(bows) = graph.with_handle(wall, index % 2, pointer.y) else {
                return Some(Drag::refused());
            };
            let value = match wall {
                Wall::Hub => bows.hub,
                Wall::Shroud => bows.shroud,
            };
            let value = if index % 2 == 0 { value.0 } else { value.1 };
            let written = inputs.blade.bows != bows;
            inputs.blade.bows = bows;
            let third = if index % 2 == 0 { "1/3" } else { "2/3" };
            Some(Drag {
                status: format!("{} {third}: desviación = {value:.1}°", wall.name()),
                written,
            })
        }
        4 => {
            let applied = applied_incidence_deg(inputs);
            let clamped = incidence_for_beta1(pointer.y, graph.shroud.beta.0.to_degrees(), applied);
            let shift = (clamped.value - applied).to_radians();
            if !graph.is_valid_with(&graph.bows, shift, None) {
                return Some(Drag::refused());
            }
            let mut status = format!("Incidencia = {:.1}°", clamped.value);
            if clamped.limited {
                status.push_str(&format!(
                    " (límite: la incidencia va de {} a {}°)",
                    INCIDENCE_RANGE_DEG.0, INCIDENCE_RANGE_DEG.1
                ));
            }
            let text = bow_text(clamped.value);
            let written = inputs.incidence != text;
            inputs.incidence = text;
            Some(Drag { status, written })
        }
        _ => {
            let clamped = beta2_for_target(pointer.y);
            if !graph.is_valid_with(&graph.bows, 0.0, Some(clamped.value.to_radians())) {
                return Some(Drag::refused());
            }
            let mut status = format!("β2 fijado = {:.1}°", clamped.value);
            if clamped.limited {
                status.push_str(" (límite: de 1 a 89°)");
            }
            let text = beta2_text(clamped.value);
            let written = inputs.beta2 != text;
            inputs.beta2 = text;
            Some(Drag { status, written })
        }
    }
}

/// Lo que pasó en un fotograma de arrastre.
struct Drag {
    /// Lo que dice la línea bajo la gráfica.
    status: String,
    /// Si se escribió una caja (y hay que recalcular).
    written: bool,
}

impl Drag {
    /// El punto no se mueve: con la desviación actual β saldría del rango.
    fn refused() -> Self {
        Self {
            status: format!(
                "No se mueve: con la desviación actual β saldría de {BLADE_BETA_MIN_DEG}° a 90°"
            ),
            written: false,
        }
    }
}

/// Solo para las capturas: `HMF_BLADE="8,-5"` pone la desviación de las dos
/// manijas, en grados, `HMF_STREAMLINES=7` el número de líneas de corriente y
/// `HMF_SHAPE=free_3d` la forma del álabe.
pub(super) fn seed_from_env(inputs: &mut Inputs) {
    if let Ok(text) = std::env::var("HMF_SHAPE") {
        match hm_design::blade_setup::BladeShape::from_id(text.trim()) {
            Some(shape) => inputs.blade.shape = shape,
            None => eprintln!("HMF_SHAPE: se espera free_3d, ruled_3d, free_2d…; llegó {text:?}"),
        }
    }
    if let Ok(text) = std::env::var("HMF_STREAMLINES") {
        match crate::blade_inlet::streamlines_from_text(&text) {
            Some(count) => inputs.blade.streamlines = count,
            None => eprintln!("HMF_STREAMLINES: se espera 3, 5, 7 o 9; llegó {text:?}"),
        }
    }
    let Ok(text) = std::env::var("HMF_BLADE") else {
        return;
    };
    let parsed: Vec<f64> = text
        .split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    let [first, second] = parsed[..] else {
        eprintln!("HMF_BLADE: se esperan dos números en grados, «8,-5»; llegó {text:?}");
        return;
    };
    inputs.beta_bow1 = bow_text(first);
    inputs.beta_bow2 = bow_text(second);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(bow: (f64, f64)) -> BetaGraph {
        BetaGraph {
            r: (0.04, 0.105),
            beta: (20.3_f64.to_radians(), 18.8_f64.to_radians()),
            bow,
        }
    }

    #[test]
    fn curve_with_zero_bow_is_the_straight_line() {
        let g = graph((0.0, 0.0));
        let [a, b] = g.straight();
        for p in g.curve(21) {
            let t = (p[0] - a[0]) / (b[0] - a[0]);
            let expected = a[1] + (b[1] - a[1]) * t;
            assert!((p[1] - expected).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn handles_sit_at_a_third_and_two_thirds_of_r() {
        let g = graph((3.0, -2.0));
        let h = g.handles();
        assert!((h[0][0] - (40.0 + 65.0 / 3.0)).abs() < 1e-9);
        assert!((h[1][0] - (40.0 + 130.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn dragging_a_handle_gives_the_matching_bow() {
        let g = graph((7.0, -3.0));
        let h = g.handles();
        assert!((g.bow_from_handle(0, h[0][1]) - 7.0).abs() < 1e-9);
        assert!((g.bow_from_handle(1, h[1][1]) + 3.0).abs() < 1e-9);
    }

    #[test]
    fn dragging_beta1_to_a_reachable_value_sets_the_incidence() {
        let c = incidence_for_beta1(21.0, 20.0, 2.0);
        assert!((c.value - 3.0).abs() < 1e-9);
        assert!(!c.limited);
    }

    #[test]
    fn dragging_beta1_beyond_the_incidence_range_clamps() {
        let high = incidence_for_beta1(60.0, 20.0, 2.0);
        assert_eq!((high.value, high.limited), (INCIDENCE_RANGE_DEG.1, true));
        let low = incidence_for_beta1(5.0, 20.0, 2.0);
        assert_eq!((low.value, low.limited), (INCIDENCE_RANGE_DEG.0, true));
    }

    #[test]
    fn beta2_target_is_kept_between_one_and_eighty_nine_degrees() {
        assert_eq!(
            beta2_for_target(24.0),
            Clamped {
                value: 24.0,
                limited: false
            }
        );
        assert_eq!(
            beta2_for_target(-5.0),
            Clamped {
                value: 1.0,
                limited: true
            }
        );
        assert_eq!(
            beta2_for_target(120.0),
            Clamped {
                value: 89.0,
                limited: true
            }
        );
        assert_eq!(beta2_text(24.0), "24.00 deg");
    }

    #[test]
    fn dragging_a_handle_never_drives_beta_under_the_minimum() {
        // La otra manija ya está en −30°: la primera no puede llegar a −30°.
        let g = graph((0.0, -30.0));
        let bow = g.bow_from_handle(0, -60.0);
        let low = BetaGraph {
            bow: (bow, -30.0),
            ..g
        };
        let min = low
            .curve(200)
            .iter()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min);
        assert!(min >= BLADE_BETA_MIN_DEG - 1e-6, "{min}");
        // Y no recorta más de lo necesario.
        assert!(bow < 0.0 && bow > BETA_BOW_RANGE_DEG.0);
    }

    fn snap(bow1: &str) -> BetaSnapshot {
        BetaSnapshot {
            bow1: bow1.to_string(),
            ..BetaSnapshot::default()
        }
    }

    #[test]
    fn straighten_clears_only_the_bows() {
        let mut inputs = Inputs {
            beta_bow1: "5 deg".into(),
            beta_bow2: "-3 deg".into(),
            incidence: "4 deg".into(),
            beta2: "24 deg".into(),
            ..Inputs::default()
        };
        straighten(&mut inputs);
        assert_eq!(
            (inputs.beta_bow1.as_str(), inputs.beta_bow2.as_str()),
            ("", "")
        );
        assert_eq!(
            (inputs.incidence.as_str(), inputs.beta2.as_str()),
            ("4 deg", "24 deg")
        );
    }

    #[test]
    fn undo_restores_the_previous_snapshot_and_redo_returns() {
        let mut history = BetaHistory::default();
        history.follow(&snap(""));
        history.record(&snap("1 deg"), 10.0);
        history.record(&snap("2 deg"), 20.0);
        let back = history.undo(&snap("2 deg")).unwrap();
        assert_eq!(back, snap("1 deg"));
        let forward = history.redo(&back).unwrap();
        assert_eq!(forward, snap("2 deg"));
    }

    #[test]
    fn undo_restores_the_previous_shape_and_count() {
        use hm_design::blade_setup::{BladeSetup, BladeShape};
        let mut inputs = Inputs::default();
        let before = inputs.blade.clone();
        let mut history = BetaHistory::default();
        history.follow(&BetaSnapshot::of(&inputs));
        inputs.blade = BladeSetup {
            shape: BladeShape::Ruled3D,
            streamlines: 9,
            ..BladeSetup::default()
        };
        let after = inputs.blade.clone();
        history.record(&BetaSnapshot::of(&inputs), 10.0);
        let back = history.undo(&BetaSnapshot::of(&inputs)).unwrap();
        back.apply(&mut inputs);
        assert_eq!(inputs.blade, before);
        assert_eq!(inputs.blade.streamlines, 5);
        let forward = history.redo(&BetaSnapshot::of(&inputs)).unwrap();
        forward.apply(&mut inputs);
        assert_eq!(inputs.blade, after);
    }

    #[test]
    fn changes_close_in_time_are_one_step() {
        let mut history = BetaHistory::default();
        history.follow(&snap(""));
        history.record(&snap("1 deg"), 10.0);
        history.record(&snap("2 deg"), 10.1);
        history.record(&snap("3 deg"), 10.2);
        assert_eq!(history.undo(&snap("3 deg")).unwrap(), snap(""));
        assert!(!history.can_undo());
    }

    #[test]
    fn a_new_record_clears_the_redo_stack() {
        let mut history = BetaHistory::default();
        history.follow(&snap(""));
        history.record(&snap("1 deg"), 10.0);
        let back = history.undo(&snap("1 deg")).unwrap();
        assert!(history.can_redo());
        history.record(&snap("9 deg"), 20.0);
        assert!(!history.can_redo());
        let _ = back;
    }

    #[test]
    fn undo_on_an_empty_history_is_none() {
        let mut history = BetaHistory::default();
        assert!(history.undo(&snap("")).is_none());
        assert!(history.redo(&snap("")).is_none());
    }

    #[test]
    fn the_reading_flags_a_loading_above_the_allowable_plus_the_margin() {
        let (ok, flagged) = reading_text(20.3, 18.8, 99.0, 0.9, 1.0, true);
        assert!(!flagged);
        assert!(ok.contains("20.3") && ok.contains("18.8") && ok.contains("99"));
        let (_, flagged) = reading_text(20.3, 18.8, 99.0, 1.2, 1.0, true);
        assert!(flagged);
    }

    #[test]
    fn the_reading_does_not_flag_the_loading_of_a_non_cylindrical_blade() {
        let (_, flagged) = reading_text(20.3, 18.8, 99.0, 1.2, 1.0, false);
        assert!(!flagged);
    }

    #[test]
    fn the_clamped_bow_is_accepted_by_the_engine_range_for_any_drag_target() {
        // Con el muestreo del motor, no con uno propio de la gráfica.
        let g = graph((0.0, -30.0));
        let mut target = -60.0;
        while target <= 60.0 {
            for index in 0..2 {
                let bow = g.bow_from_handle(index, target);
                let bow = if index == 0 { (bow, -30.0) } else { (0.0, bow) };
                let drawn = BetaGraph { bow, ..g };
                assert!(
                    drawn.is_valid(),
                    "manija {index}, objetivo {target}: {bow:?}"
                );
            }
            target += 0.7;
        }
    }

    #[test]
    fn an_end_drag_that_breaks_the_range_is_refused() {
        // Con −12° en la primera manija, β ya baja unos 5° bajo la recta.
        let g = graph((-12.0, 0.0));
        assert!(g.is_valid());
        assert!(!g.with_beta2_deg(4.0).is_valid());
        assert!(g.with_beta2_deg(18.0).is_valid());
        assert!(!g.with_beta1_deg(6.0).is_valid());
        assert!(g.with_beta1_deg(20.0).is_valid());
        // Sin desviación el motor no comprueba nada, y la gráfica tampoco.
        assert!(graph((0.0, 0.0)).with_beta2_deg(4.0).is_valid());
    }

    #[test]
    fn the_incidence_text_is_read_with_its_unit() {
        assert!((incidence_text_deg("0.05 rad") - 2.864_788_975).abs() < 1e-6);
        assert_eq!(incidence_text_deg("4 deg"), 4.0);
        let default = hm_design::geometry::DEFAULT_INCIDENCE_DEG;
        assert_eq!(incidence_text_deg(""), default);
        assert_eq!(incidence_text_deg("no es un número"), default);
    }

    #[test]
    fn dragging_beyond_the_range_clamps_the_bow() {
        let g = graph((0.0, 0.0));
        assert_eq!(g.bow_from_handle(0, 80.0), BETA_BOW_RANGE_DEG.1);
        assert_eq!(g.bow_from_handle(1, -80.0), BETA_BOW_RANGE_DEG.0);
    }

    // -----------------------------------------------------------------
    // Forma 3D
    // -----------------------------------------------------------------

    use crate::model::Basis;
    use hm_design::blade_camber::BladeCamber;
    use hm_design::blade_setup::{BladeShape, CamberBows};

    /// B1 con esa forma y esas manijas: el resultado y su línea media.
    fn b1_3d(shape: BladeShape, bows: CamberBows) -> (Outcome, BladeCamber) {
        let mut inputs = Inputs::default();
        inputs.blade.shape = shape;
        inputs.blade.streamlines = 5;
        inputs.blade.bows = bows;
        let outcome = crate::model::compute(&inputs, &Basis::default()).unwrap();
        let camber = crate::blade_inlet::camber_of(&outcome)
            .unwrap()
            .expect("forma 3D");
        (outcome, camber)
    }

    fn flat() -> CamberBows {
        CamberBows::default()
    }

    /// β de la recta de la pared en `x`, grados.
    fn wall_line_deg(camber: &BladeCamber, wall: Wall, x: f64) -> f64 {
        let line = match wall {
            Wall::Hub => camber.lines.first(),
            Wall::Shroud => camber.lines.last(),
        }
        .unwrap();
        let (a, b) = (
            line.points.first().unwrap().beta,
            line.points.last().unwrap().beta,
        );
        (a + (b - a) * x).to_degrees()
    }

    #[test]
    fn the_3d_graph_has_one_curve_per_line_from_zero_to_one() {
        let (outcome, camber) = b1_3d(BladeShape::Free3D, flat());
        let graph = BetaGraph3d::of(&outcome, &camber);
        let curves = graph.curves();
        assert_eq!(curves.len(), 5);
        let beta2 = outcome.result.selection.beta2_deg;
        for (curve, line) in curves.iter().zip(&camber.lines) {
            assert_eq!(curve.first().unwrap()[0], 0.0);
            assert_eq!(curve.last().unwrap()[0], 1.0);
            assert!((curve.first().unwrap()[1] - line.points[0].beta.to_degrees()).abs() < 1e-9);
            assert!((curve.last().unwrap()[1] - beta2).abs() < 1e-6, "{curve:?}");
        }
        // El primer β de cada línea es su β1B (la tabla de entrada).
        let (_, triangles) = crate::blade_inlet::inlet_lines(&outcome).unwrap();
        for (curve, triangle) in curves.iter().zip(&triangles) {
            assert!((curve[0][1] - triangle.beta1b.to_degrees()).abs() < 1e-6);
        }
    }

    #[test]
    fn a_hub_handle_sits_on_the_hub_law() {
        let bows = CamberBows {
            hub: (4.0, 0.0),
            shroud: (0.0, 0.0),
        };
        let (outcome, camber) = b1_3d(BladeShape::Free3D, bows);
        let graph = BetaGraph3d::of(&outcome, &camber);
        let [first, second] = graph.handles(Wall::Hub);
        let line = wall_line_deg(&camber, Wall::Hub, 1.0 / 3.0);
        assert!((first[0] - 1.0 / 3.0).abs() < 1e-9);
        // La manija es el punto de control: la recta más la desviación.
        assert!((first[1] - (line + 4.0)).abs() < 1e-9, "{first:?}");
        assert!((second[0] - 2.0 / 3.0).abs() < 1e-9);
        // La cúbica vale 4/9 de la manija en su x: ahí está la curva del cubo.
        let on_curve = graph.curves()[0]
            .iter()
            .min_by(|a, b| {
                (a[0] - 1.0 / 3.0)
                    .abs()
                    .total_cmp(&(b[0] - 1.0 / 3.0).abs())
            })
            .copied()
            .unwrap();
        let x = on_curve[0];
        let expected = wall_line_deg(&camber, Wall::Hub, x) + 4.0 * 3.0 * (1.0 - x) * (1.0 - x) * x;
        assert!(
            (on_curve[1] - expected).abs() < 1e-6,
            "{on_curve:?} {expected}"
        );
    }

    #[test]
    fn dragging_a_hub_handle_writes_only_the_hub() {
        let start = CamberBows {
            hub: (2.0, -1.0),
            shroud: (0.5, 3.0),
        };
        let (outcome, camber) = b1_3d(BladeShape::Free3D, start);
        let graph = BetaGraph3d::of(&outcome, &camber);
        let wanted = wall_line_deg(&camber, Wall::Hub, 1.0 / 3.0) + 3.0;
        let bows = graph
            .with_handle(Wall::Hub, 0, wanted)
            .expect("es una desviación válida");
        assert_eq!(bows.hub, (3.0, -1.0));
        assert_eq!(bows.shroud, start.shroud);
        // Con las manijas puestas, la manija vuelve a estar donde se pidió.
        let (again, camber) = b1_3d(BladeShape::Free3D, bows);
        let handle = BetaGraph3d::of(&again, &camber).handles(Wall::Hub)[0];
        assert!((handle[1] - wanted).abs() < 1e-6, "{handle:?} {wanted}");
    }

    #[test]
    fn a_handle_is_clamped_to_its_range() {
        let (outcome, camber) = b1_3d(BladeShape::Free3D, flat());
        let graph = BetaGraph3d::of(&outcome, &camber);
        let wanted = wall_line_deg(&camber, Wall::Hub, 1.0 / 3.0) + 50.0;
        let bows = graph.with_handle(Wall::Hub, 0, wanted).unwrap();
        assert_eq!(bows.hub.0, BETA_BOW_RANGE_DEG.1);
    }

    // Review Focus 4: un arrastre que sacaría β de 5° a 90° no se escribe.
    #[test]
    fn a_drag_that_breaks_the_wall_is_refused() {
        let start = CamberBows {
            hub: (-30.0, 0.0),
            shroud: (0.0, 0.0),
        };
        let (outcome, camber) = b1_3d(BladeShape::Free3D, start);
        let graph = BetaGraph3d::of(&outcome, &camber);
        // Pedir lo mismo que ya hay es válido…
        let same = wall_line_deg(&camber, Wall::Hub, 2.0 / 3.0);
        assert!(graph.with_handle(Wall::Hub, 1, same).is_some());
        // …y llevar la segunda manija a −30° deja el cubo bajo 5°.
        assert_eq!(graph.with_handle(Wall::Hub, 1, same - 60.0), None);
    }

    #[test]
    fn undo_restores_the_four_handles() {
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free3D;
        inputs.blade.bows = CamberBows {
            hub: (1.0, 2.0),
            shroud: (3.0, 4.0),
        };
        let before = inputs.blade.bows;
        let mut history = BetaHistory::default();
        history.follow(&BetaSnapshot::of(&inputs));
        inputs.blade.bows.hub.0 = 9.0;
        history.record(&BetaSnapshot::of(&inputs), 10.0);
        let back = history.undo(&BetaSnapshot::of(&inputs)).unwrap();
        back.apply(&mut inputs);
        assert_eq!(inputs.blade.bows, before);
    }

    #[test]
    fn straighten_3d_zeroes_the_handles_and_leaves_the_2d_boxes() {
        let mut inputs = Inputs {
            beta_bow1: "5 deg".into(),
            beta_bow2: "-3 deg".into(),
            ..Inputs::default()
        };
        inputs.blade.bows = CamberBows {
            hub: (1.0, 2.0),
            shroud: (3.0, 4.0),
        };
        straighten_3d(&mut inputs);
        assert_eq!(inputs.blade.bows, CamberBows::default());
        assert_eq!(inputs.beta_bow1, "5 deg");
        assert_eq!(inputs.beta_bow2, "-3 deg");
    }

    #[test]
    fn with_the_2d_shape_the_graph_is_todays() {
        let outcome = crate::model::compute(&Inputs::default(), &Basis::default()).unwrap();
        assert_eq!(crate::blade_inlet::camber_of(&outcome), Ok(None));
        let graph = graph_of(&outcome);
        let curve = graph.curve(10);
        // La abscisa sigue siendo r en mm, de D1/2 a D2/2.
        assert!((curve[0][0] - outcome.result.d1.si / 2.0 * 1000.0).abs() < 1e-9);
        assert!((curve[10][0] - outcome.result.d2.si / 2.0 * 1000.0).abs() < 1e-9);
        assert_eq!(graph.bow, outcome.result.selection.blade_bow_deg);
    }
}
