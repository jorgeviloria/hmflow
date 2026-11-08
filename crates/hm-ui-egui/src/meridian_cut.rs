//! El punto de corte del cubo, arrastrable: solo se mueve en z, con los gestos
//! de precisión de los demás puntos (Mayús fija el eje, Alt afina, flechas, y
//! una lectura en vivo). El recorte se crea desde su caja; aquí solo se mueve.

use eframe::egui;
use egui_plot::{PlotPoint, Points, Text};
use hm_design::meridian::{CUT_CONTROL, CUT_NAME, MeridionalModel};
use hm_project::Meridian;

use crate::meridian::{meridian_color, selected_control};

/// Paso de las flechas, en mm; con Alt, la décima parte.
const STEP: f64 = 1.0;
const FINE_STEP: f64 = 0.1;
/// Cuánto afina Alt un arrastre.
const FINE_SCALE: f64 = 0.1;

/// Con Mayús (`lock`) la z no sale de donde empezó el arrastre.
pub(super) fn constrain_z(pointer_z: f64, anchor_z: f64, lock: bool) -> f64 {
    if lock { anchor_z } else { pointer_z }
}

/// Un paso de las flechas: 1 mm, o 0,1 mm con Alt (`fine`), a la rejilla de 0,01 mm.
pub(super) fn nudge_z(z: f64, steps: i32, fine: bool) -> f64 {
    let step = if fine { FINE_STEP } else { STEP };
    grid(z + f64::from(steps) * step)
}

/// La lectura junto al punto mientras se mueve, con coma decimal.
pub(super) fn readout(z: f64, rc: f64) -> String {
    format!("corte z = {z:.3} mm · r = {rc:.1} mm").replace('.', ",")
}

/// Donde cae el punto al arrastrar: el puntero, afinado con Alt (la décima
/// parte del recorrido desde `anchor_z`) y fijo en `anchor_z` con Mayús.
pub(super) fn dragged_z(pointer_z: f64, anchor_z: f64, lock: bool, fine: bool) -> f64 {
    let scaled = if fine {
        anchor_z + (pointer_z - anchor_z) * FINE_SCALE
    } else {
        pointer_z
    };
    constrain_z(scaled, anchor_z, lock)
}

/// z a la rejilla de 0,01 mm, sin «-0».
fn grid(value: f64) -> f64 {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 { 0.0 } else { rounded }
}

/// Recorta z a [0, `edge_z`], el borde de ataque sobre el cubo. Con `min` y
/// `max` y no `clamp`, que entra en pánico si el borde quedara por debajo de 0.
pub(super) fn limit_z(z: f64, edge_z: f64) -> f64 {
    z.min(edge_z).max(0.0)
}

/// Como `grid` pero hacia abajo: nunca pasa de `value`.
fn grid_floor(value: f64) -> f64 {
    let floored = (value * 100.0 + 1e-9).floor() / 100.0;
    if floored == 0.0 { 0.0 } else { floored }
}

/// Escribe z en la caja del recorte. Devuelve si cambió (el valor, no el
/// texto: «30 mm» y «30.00 mm» son lo mismo). Al tope del borde de ataque
/// redondea hacia abajo, para que el texto nunca pase del borde.
pub(super) fn write_z(boxes: &mut Meridian, z: f64, edge_z: f64) -> bool {
    let limited = limit_z(z, edge_z);
    let value = if limited >= edge_z {
        grid_floor(limited)
    } else {
        grid(limited)
    };
    let text = format!("{value:.2} mm");
    let before = hm_engine::meridian_edits_of(boxes).hub_cut.z;
    let changed = before != Some(value);
    boxes.hub_cut.z = text;
    changed
}

/// El borde de ataque sobre el cubo: el tope de z del corte.
fn edge_z(model: &MeridionalModel) -> f64 {
    model.params.edge_points[1][0]
}

fn anchor_id() -> egui::Id {
    egui::Id::unique("meridian_cut_anchor")
}

/// La z del corte al empezar a arrastrarlo, para Mayús y Alt.
pub(super) fn start_drag(ctx: &egui::Context, model: &MeridionalModel) {
    if let Some([z, _]) = model.cut_handle() {
        ctx.data_mut(|data| data.insert_temp(anchor_id(), z));
    }
}

/// Selecciona el punto de corte (para las capturas con `HMF_SELECT=cut`).
pub(super) fn select(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(crate::meridian::selected_id(), CUT_CONTROL));
}

/// El punto de corte, con el color de su curva.
pub(super) fn draw(plot_ui: &mut egui_plot::PlotUi<'_>, model: &MeridionalModel) {
    if let Some(at) = model.cut_handle() {
        plot_ui.points(
            Points::new(CUT_NAME, vec![at])
                .radius(4.8)
                .color(meridian_color(CUT_NAME)),
        );
    }
}

/// Un paso del arrastre del corte: solo z, con Mayús fijo y Alt afinado, y la
/// lectura junto al punto. Devuelve si cambió la caja.
pub(super) fn drag(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    boxes: &mut Meridian,
    pointer_z: f64,
) -> bool {
    let Some([current, rc]) = model.cut_handle() else {
        return false;
    };
    let ctx = plot_ui.ctx().clone();
    let anchor = ctx
        .data(|data| data.get_temp::<f64>(anchor_id()))
        .unwrap_or(current);
    let (lock, fine) = ctx.input(|input| (input.modifiers.shift, input.modifiers.alt));
    let z = limit_z(dragged_z(pointer_z, anchor, lock, fine), edge_z(model));
    plot_ui.text(
        Text::new(CUT_NAME, PlotPoint::new(z, rc), readout(z, rc))
            .anchor(egui::Align2::LEFT_BOTTOM),
    );
    write_z(boxes, z, edge_z(model))
}

/// Las flechas mueven el corte si es el punto seleccionado y no se está
/// escribiendo en una caja: 1 mm, o 0,1 mm con Alt.
pub(super) fn arrows(
    plot_ui: &egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    boxes: &mut Meridian,
) -> bool {
    use egui::Key;
    let ctx = plot_ui.ctx();
    let Some([z, _]) = model.cut_handle() else {
        return false;
    };
    if selected_control(ctx) != CUT_CONTROL || ctx.memory(|memory| memory.focused().is_some()) {
        return false;
    }
    let (steps, fine) = ctx.input_mut(|input| {
        let mut steps = 0;
        for (key, step) in [
            (Key::ArrowRight, 1),
            (Key::ArrowUp, 1),
            (Key::ArrowLeft, -1),
            (Key::ArrowDown, -1),
        ] {
            while input.consume_key(egui::Modifiers::NONE, key)
                || input.consume_key(egui::Modifiers::ALT, key)
            {
                steps += step;
            }
        }
        (steps, input.modifiers.alt)
    });
    steps != 0 && write_z(boxes, nudge_z(z, steps, fine), edge_z(model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meridian::History;

    #[test]
    fn constrain_z_locks_to_the_anchor_when_shift() {
        assert_eq!(constrain_z(41.0, 37.5, true), 37.5);
        assert_eq!(constrain_z(41.0, 37.5, false), 41.0);
        // Alt afina el recorrido a la décima parte.
        assert!((dragged_z(47.5, 37.5, false, true) - 38.5).abs() < 1e-12);
        assert_eq!(dragged_z(47.5, 37.5, true, true), 37.5);
    }

    #[test]
    fn nudge_z_steps_one_mm_and_a_tenth_when_fine() {
        assert_eq!(nudge_z(37.5, 1, false), 38.5);
        assert_eq!(nudge_z(37.5, -2, false), 35.5);
        assert_eq!(nudge_z(37.5, 1, true), 37.6);
        assert_eq!(nudge_z(37.5, -1, true), 37.4);
    }

    #[test]
    fn readout_formats_z_and_r() {
        assert_eq!(readout(37.5, 53.2), "corte z = 37,500 mm · r = 53,2 mm");
    }

    #[test]
    fn dragging_the_cut_writes_z_and_never_passes_the_edge() {
        let mut boxes = Meridian::default();
        boxes.hub_cut.z = "30 mm".into();
        assert!(write_z(&mut boxes, 35.123, 40.0));
        assert_eq!(boxes.hub_cut.z, "35.12 mm");
        assert!(
            !write_z(&mut boxes, 35.123, 40.0),
            "mismo valor, sin cambio"
        );
        // Más allá del borde de ataque: se queda en el borde.
        assert!(write_z(&mut boxes, 99.0, 40.0));
        assert_eq!(boxes.hub_cut.z, "40.00 mm");
        // Antes de la entrada del cubo: en 0.
        write_z(&mut boxes, -5.0, 40.0);
        assert_eq!(boxes.hub_cut.z, "0.00 mm");
        // Un borde por debajo de 0 no entra en pánico.
        write_z(&mut boxes, 10.0, -1.0);
        assert_eq!(boxes.hub_cut.z, "0.00 mm");
    }

    #[test]
    fn z_at_a_non_round_edge_is_rounded_down_and_stays_valid() {
        let mut boxes = Meridian::default();
        write_z(&mut boxes, 99.0, 37.456);
        assert_eq!(boxes.hub_cut.z, "37.45 mm");
        let z = hm_engine::meridian_edits_of(&boxes).hub_cut.z.unwrap();
        assert!(z <= 37.456, "{z}");
        // Por debajo del borde sigue redondeando al más cercano.
        write_z(&mut boxes, 30.006, 37.456);
        assert_eq!(boxes.hub_cut.z, "30.01 mm");
    }

    #[test]
    fn writing_the_same_value_in_other_text_is_not_a_change() {
        let mut boxes = Meridian::default();
        boxes.hub_cut.z = "30 mm".into();
        assert!(!write_z(&mut boxes, 30.0, 40.0));
        assert_eq!(boxes.hub_cut.z, "30.00 mm");
        assert!(write_z(&mut boxes, 31.0, 40.0));
    }

    #[test]
    fn one_drag_is_one_undo_step() {
        let mut history = History::default();
        let mut boxes = Meridian::default();
        boxes.hub_cut.z = "30 mm".into();
        history.follow(&boxes);
        for (time, z) in [(1.0, 31.0), (1.05, 32.5), (1.1, 34.0), (1.15, 36.0)] {
            if write_z(&mut boxes, z, 40.0) {
                history.record(&boxes, time);
            }
        }
        assert_eq!(boxes.hub_cut.z, "36.00 mm");
        assert!(history.undo(&mut boxes));
        assert_eq!(boxes.hub_cut.z, "30 mm");
        assert!(!history.can_undo());
    }
}
