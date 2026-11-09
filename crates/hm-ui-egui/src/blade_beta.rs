//! Gráfica β(r) del álabe: la ley de ángulo con sus manijas arrastrables.

use eframe::egui;
use egui::Color32;
use egui_plot::{Plot, PlotPoint, Points};
use hm_design::geometry::{BLADE_BETA_MIN_DEG, beta_profile};
use hm_design::{BETA_BOW_RANGE_DEG, INCIDENCE_RANGE_DEG};

use crate::curves::line;
use crate::model::{Inputs, Outcome};

/// Fracción de r de cada manija interior.
const HANDLE_FRACTIONS: [f64; 2] = [1.0 / 3.0, 2.0 / 3.0];

/// Radio, en píxeles, dentro del cual se agarra una manija.
const GRAB_PIXELS: f32 = 14.0;

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
    fn r_mm(&self, fraction: f64) -> f64 {
        (self.r.0 + (self.r.1 - self.r.0) * fraction) * 1000.0
    }

    fn line_deg(&self, fraction: f64) -> f64 {
        (self.beta.0 + (self.beta.1 - self.beta.0) * fraction).to_degrees()
    }

    /// β mínimo de la curva con esa desviación, en grados.
    fn min_beta_deg(&self, bow: (f64, f64)) -> f64 {
        (0..=60)
            .map(|index| {
                let fraction = f64::from(index) / 60.0;
                beta_profile(self.beta.0, self.beta.1, bow, fraction).to_degrees()
            })
            .fold(f64::INFINITY, f64::min)
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
        let mut bow = wanted.clamp(BETA_BOW_RANGE_DEG.0, BETA_BOW_RANGE_DEG.1);
        // β no baja del mínimo: se acerca la manija a la recta hasta que cumpla.
        let with = |value: f64| {
            if index == 0 {
                (value, self.bow.1)
            } else {
                (self.bow.0, value)
            }
        };
        while bow != 0.0 && self.min_beta_deg(with(bow)) < BLADE_BETA_MIN_DEG {
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
pub(super) fn blade_beta_view(ui: &mut egui::Ui, outcome: &Outcome, inputs: &mut Inputs) -> bool {
    let graph = graph_of(outcome);
    ui.label(
        egui::RichText::new(
            "β(r) del álabe, en grados. Gris: la recta del 1D. Naranjas: dan la forma. \
             Azules: los extremos; β1 mueve la incidencia y β2 se fija a mano (vacía su \
             caja para volver a Euler).",
        )
        .small()
        .weak(),
    );
    let mut edited = false;
    let status_id = egui::Id::unique("blade_beta_status");
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
            plot_ui.line(line("Recta del 1D", vec![a, b], Color32::GRAY).width(1.0));
            plot_ui.line(line(
                "β(r)",
                graph.curve(60),
                Color32::from_rgb(70, 150, 240),
            ));
            let handles = graph.handles();
            // Polígono de control, para que se vea qué tira de la curva.
            plot_ui.line(
                line(
                    "",
                    vec![a, handles[0], handles[1], b],
                    Color32::from_rgb(240, 160, 50),
                )
                .width(1.0)
                .style(egui_plot::LineStyle::dashed_loose()),
            );
            plot_ui.points(
                Points::new("Manijas", handles.to_vec())
                    .color(Color32::from_rgb(240, 160, 50))
                    .radius(6.0),
            );
            plot_ui.points(
                Points::new("Extremos", graph.ends().to_vec())
                    .color(Color32::from_rgb(70, 150, 240))
                    .radius(6.0),
            );
            if let Some(status) = drag_handles(plot_ui, &graph, inputs) {
                plot_ui
                    .ctx()
                    .data_mut(|data| data.insert_temp(status_id, status));
                edited = true;
            }
        });
    if let Some(status) = ui.ctx().data(|data| data.get_temp::<String>(status_id)) {
        ui.label(egui::RichText::new(status).small());
    }
    edited
}

/// La incidencia que lleva la caja, o la de por defecto si está vacía.
fn applied_incidence_deg(inputs: &Inputs) -> f64 {
    inputs
        .incidence
        .split_whitespace()
        .next()
        .and_then(|number| number.parse().ok())
        .unwrap_or(hm_design::geometry::DEFAULT_INCIDENCE_DEG)
}

/// Agarra el punto más cercano al empezar a arrastrar y, mientras dura,
/// escribe en su caja lo que da el puntero: la desviación (manijas 0 y 1), la
/// incidencia (2, β1) o el β2 fijado (3). Devuelve lo que dice la lectura.
fn drag_handles(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    graph: &BetaGraph,
    inputs: &mut Inputs,
) -> Option<String> {
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
            let mut status = format!("β2 fijado = {:.1}°", clamped.value);
            if clamped.limited {
                status.push_str(" (límite: de 1 a 89°)");
            }
            (&mut inputs.beta2, beta2_text(clamped.value), status)
        }
    };
    let changed = *target != text;
    *target = text;
    changed.then_some(status)
}

/// Solo para las capturas: `HMF_BLADE="8,-5"` pone la desviación de las dos
/// manijas, en grados.
pub(super) fn seed_from_env(inputs: &mut Inputs) {
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
        let low = BetaGraph { bow: (bow, -30.0), ..g };
        let min = low.curve(200).iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        assert!(min >= BLADE_BETA_MIN_DEG - 1e-6, "{min}");
        // Y no recorta más de lo necesario.
        assert!(bow < 0.0 && bow > BETA_BOW_RANGE_DEG.0);
    }

    #[test]
    fn dragging_beyond_the_range_clamps_the_bow() {
        let g = graph((0.0, 0.0));
        assert_eq!(g.bow_from_handle(0, 80.0), BETA_BOW_RANGE_DEG.1);
        assert_eq!(g.bow_from_handle(1, -80.0), BETA_BOW_RANGE_DEG.0);
    }
}
