//! Pestaña Geometría: la vista en planta, dibujada desde `PlanModel`.

use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Legend, Plot};

use crate::curves::line;
use crate::model::Outcome;

/// Cómo se dibuja una curva del modelo en planta.
struct CurveStyle<'a> {
    legend: &'a str,
    color: Color32,
    width: f32,
    dashed: bool,
}

/// Estilo por el nombre de la curva. Una curva que el modelo traiga sin
/// estilo asignado se dibuja igual, con su nombre: no desaparece.
fn curve_style(name: &str) -> CurveStyle<'_> {
    let style = |legend, color, width, dashed| CurveStyle {
        legend,
        color,
        width,
        dashed,
    };
    let numbered_blade = name
        .strip_prefix("Álabe ")
        .is_some_and(|number| number.parse::<u32>().is_ok());
    match name {
        "D1" | "D2" => style(name, Color32::GRAY, 1.0, false),
        "Lengua" => style(name, Color32::from_gray(90), 1.0, true),
        "Voluta" => style(name, Color32::from_rgb(240, 160, 50), 2.0, false),
        _ if numbered_blade => style("Álabes", Color32::from_rgb(70, 150, 240), 2.0, false),
        _ => style(name, Color32::from_rgb(120, 180, 90), 2.0, false),
    }
}

pub(super) fn plan_view(ui: &mut egui::Ui, outcome: &Outcome) {
    let Ok(shape) = &outcome.geometry else {
        ui.label("Sin geometría para este punto.");
        return;
    };
    let result = &outcome.result;
    // El visor no calcula: dibuja el mismo modelo que se exporta.
    let model = match hm_design::plan::plan_model(result, shape) {
        Ok(model) => model,
        Err(error) => {
            ui.label(format!("Sin geometría: {error}."));
            return;
        }
    };

    ui.label(
        RichText::new(format!(
            "Vista en planta, mm. Z = {}, β1B = {:.1}°, β2 = {:.1}°, envolvimiento = {:.0}°. \
             Álabe 2D (línea media); voluta por sus radios exteriores cada 45°, interpolados. \
             Huella {}.",
            result.selection.blade_count,
            shape.impeller.beta1b.to_degrees(),
            result.selection.beta2_deg,
            shape.impeller.wrap_angle.to_degrees(),
            model.fingerprint_text()
        ))
        .small()
        .weak(),
    );
    Plot::new("plan")
        .data_aspect(1.0)
        .legend(Legend::default())
        .show(ui, |plot_ui| {
            for curve in &model.curves {
                let style = curve_style(&curve.name);
                let mut drawn =
                    line(style.legend, curve.points.clone(), style.color).width(style.width);
                if style.dashed {
                    drawn = drawn.style(egui_plot::LineStyle::dashed_loose());
                }
                plot_ui.line(drawn);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_plan_curve_keeps_the_look_it_had() {
        let grey = curve_style("D1");
        assert_eq!(
            (grey.legend, grey.color, grey.width, grey.dashed),
            ("D1", Color32::GRAY, 1.0, false)
        );
        assert_eq!(curve_style("D2").legend, "D2");
        let tongue = curve_style("Lengua");
        assert_eq!(
            (tongue.color, tongue.width, tongue.dashed),
            (Color32::from_gray(90), 1.0, true)
        );
        for name in ["Álabe 1", "Álabe 7", "Álabe 12"] {
            let blade = curve_style(name);
            assert_eq!(
                (blade.legend, blade.color, blade.width),
                ("Álabes", Color32::from_rgb(70, 150, 240), 2.0),
                "{name}"
            );
        }
        let volute = curve_style("Voluta");
        assert_eq!(
            (volute.legend, volute.color, volute.width),
            ("Voluta", Color32::from_rgb(240, 160, 50), 2.0)
        );
    }

    #[test]
    fn a_curve_without_a_style_is_still_drawn_under_its_own_name() {
        let unknown = curve_style("Difusor");
        assert_eq!(unknown.legend, "Difusor");
        assert!(unknown.width > 0.0);
        // Sin número no es uno de los Z álabes: no se agrupa con ellos.
        assert_eq!(curve_style("Álabe guía").legend, "Álabe guía");
    }
}
