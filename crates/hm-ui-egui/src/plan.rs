//! Pestaña Geometría: la vista en planta, dibujada desde `PlanModel`.

use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Legend, Plot};

use hm_design::blade_camber::BladeCamber;
use hm_design::blade_setup::BladeShape;
use hm_design::plan::PlanModel;

use crate::blade_inlet::camber_of;
use crate::curves::line;
use crate::model::Outcome;
use crate::theme::{self, Palette};

/// Cómo se dibuja una curva del modelo en planta.
struct CurveStyle<'a> {
    legend: &'a str,
    color: Color32,
    width: f32,
    dashed: bool,
}

/// Estilo por el nombre de la curva. Una curva que el modelo traiga sin
/// estilo asignado se dibuja igual, con su nombre: no desaparece.
fn curve_style<'a>(p: &Palette, name: &'a str) -> CurveStyle<'a> {
    let style = |legend, color, width, dashed| CurveStyle {
        legend,
        color,
        width,
        dashed,
    };
    // «Álabe 3», o «Álabe 3 · cubo» con la forma 3D.
    let (numbered_blade, part) = match name.strip_prefix("Álabe ") {
        Some(rest) => match rest.split_once(" · ") {
            Some((number, part)) => (number.parse::<u32>().is_ok(), Some(part)),
            None => (rest.parse::<u32>().is_ok(), None),
        },
        None => (false, None),
    };
    match (name, part) {
        ("D1" | "D2", _) => style(name, p.reference, 1.0, false),
        ("Lengua", _) => style(name, p.tongue, 1.0, true),
        ("Voluta", _) => style(name, p.volute, 2.0, false),
        _ if numbered_blade => match part {
            None | Some("cubo" | "tapa") => style("Álabes", p.blade, 2.0, false),
            Some("borde de ataque" | "borde de salida") => {
                style("Bordes del álabe", p.construction, 1.5, false)
            }
            Some(line) if line.starts_with("línea ") => {
                style("Líneas intermedias", p.reference, 1.0, false)
            }
            Some(_) => style(name, p.construction, 2.0, false),
        },
        _ => style(name, p.construction, 2.0, false),
    }
}

/// El modelo de la planta, igual que el que exporta el motor, y la línea media
/// con que se hizo. El texto del error es el que enseña la pestaña.
pub(crate) fn plan_of(outcome: &Outcome) -> Result<(PlanModel, Option<BladeCamber>), String> {
    let shape = outcome
        .geometry
        .as_ref()
        .map_err(|_| "Sin geometría para este punto.".to_string())?;
    let camber = camber_of(outcome).map_err(|message| format!("Sin álabe 3D: {message}."))?;
    // El visor no calcula: dibuja el mismo modelo que se exporta.
    let model = hm_design::plan::plan_model_with(&outcome.result, shape, camber.as_ref())
        .map_err(|error| format!("Sin geometría: {error}."))?;
    Ok((model, camber))
}

/// La leyenda de la planta, con la descripción del álabe de la forma elegida.
fn plan_caption(outcome: &Outcome, camber: Option<&BladeCamber>, fingerprint: &str) -> String {
    let result = &outcome.result;
    let volute = "voluta por sus radios exteriores cada 45°, interpolados";
    match (camber, &outcome.geometry) {
        (Some(camber), _) => {
            let first = camber.lines.first().and_then(|line| line.points.first());
            let last = camber.lines.last().and_then(|line| line.points.first());
            let wraps = camber.lines.iter().map(|line| line.wrap.to_degrees());
            let low = wraps.clone().fold(f64::INFINITY, f64::min);
            let high = wraps.fold(f64::NEG_INFINITY, f64::max);
            let kind = if camber.shape == BladeShape::Ruled3D {
                "reglado"
            } else {
                "libre"
            };
            format!(
                "Vista en planta, mm. Z = {}, β1B = {:.1}° (cubo) y {:.1}° (tapa), β2 = {:.1}°, \
                 envolvimiento de {low:.0} a {high:.0}°. Álabe 3D {kind} (superficie media); \
                 {volute}. Huella {fingerprint}.",
                result.selection.blade_count,
                first.map_or(f64::NAN, |point| point.beta.to_degrees()),
                last.map_or(f64::NAN, |point| point.beta.to_degrees()),
                result.selection.beta2_deg,
            )
        }
        (None, Ok(shape)) => format!(
            "Vista en planta, mm. Z = {}, β1B = {:.1}°, β2 = {:.1}°, envolvimiento = {:.0}°. \
             Álabe 2D (línea media); {volute}. Huella {fingerprint}.",
            result.selection.blade_count,
            shape.impeller.beta1b.to_degrees(),
            result.selection.beta2_deg,
            shape.impeller.wrap_angle.to_degrees(),
        ),
        (None, Err(_)) => String::new(),
    }
}

pub(super) fn plan_view(ui: &mut egui::Ui, outcome: &Outcome) {
    let (model, camber) = match plan_of(outcome) {
        Ok(drawn) => drawn,
        Err(message) => {
            ui.label(message);
            return;
        }
    };
    ui.label(
        RichText::new(plan_caption(
            outcome,
            camber.as_ref(),
            &model.fingerprint_text(),
        ))
        .small()
        .weak(),
    );
    let palette = theme::palette(ui.ctx());
    Plot::new("plan")
        .data_aspect(1.0)
        .legend(Legend::default())
        .show(ui, |plot_ui| {
            for curve in &model.curves {
                let style = curve_style(palette, &curve.name);
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
    use crate::theme::{DARK, LIGHT};

    #[test]
    fn plan_legend_matches_strokes() {
        for p in [&LIGHT, &DARK] {
            let grey = curve_style(p, "D1");
            assert_eq!(
                (grey.legend, grey.color, grey.width, grey.dashed),
                ("D1", p.reference, 1.0, false)
            );
            assert_eq!(curve_style(p, "D2").legend, "D2");
            let tongue = curve_style(p, "Lengua");
            assert_eq!(
                (tongue.color, tongue.width, tongue.dashed),
                (p.tongue, 1.0, true)
            );
            for name in ["Álabe 1", "Álabe 7", "Álabe 12"] {
                let blade = curve_style(p, name);
                assert_eq!(
                    (blade.legend, blade.color, blade.width),
                    ("Álabes", p.blade, 2.0),
                    "{name}"
                );
            }
            let volute = curve_style(p, "Voluta");
            assert_eq!(
                (volute.legend, volute.color, volute.width),
                ("Voluta", p.volute, 2.0)
            );
        }
    }

    #[test]
    fn a_curve_without_a_style_is_still_drawn_under_its_own_name() {
        let unknown = curve_style(&LIGHT, "Difusor");
        assert_eq!(unknown.legend, "Difusor");
        assert!(unknown.width > 0.0);
        assert_eq!(unknown.color, LIGHT.construction);
        // Sin número no es uno de los Z álabes: no se agrupa con ellos.
        assert_eq!(curve_style(&LIGHT, "Álabe guía").legend, "Álabe guía");
    }

    use crate::model::{Basis, Inputs, compute};
    use hm_design::blade_setup::CamberBows;

    fn shaped(shape: BladeShape) -> Outcome {
        let mut inputs = Inputs::default();
        inputs.blade.shape = shape;
        inputs.blade.streamlines = 5;
        compute(&inputs, &Basis::default()).unwrap()
    }

    #[test]
    fn the_3d_curves_have_a_style_and_a_legend() {
        for p in [&LIGHT, &DARK] {
            for name in ["Álabe 2 · cubo", "Álabe 2 · tapa"] {
                let blade = curve_style(p, name);
                assert_eq!(
                    (blade.legend, blade.color, blade.width),
                    ("Álabes", p.blade, 2.0),
                    "{name}"
                );
            }
            let line = curve_style(p, "Álabe 1 · línea 2");
            assert_eq!(line.legend, "Líneas intermedias");
            assert!(line.width < 2.0, "trazo fino: {}", line.width);
            for name in ["Álabe 3 · borde de ataque", "Álabe 3 · borde de salida"] {
                assert_eq!(curve_style(p, name).legend, "Bordes del álabe", "{name}");
            }
            // Lo que no es de un álabe numerado no se agrupa con ellos.
            assert_eq!(
                curve_style(p, "Álabe guía · cubo").legend,
                "Álabe guía · cubo"
            );
        }
    }

    #[test]
    fn the_3d_plan_is_the_one_the_engine_exports() {
        let outcome = shaped(BladeShape::Free3D);
        let (model, camber) = plan_of(&outcome).unwrap();
        assert!(camber.is_some());
        assert!(model.curve("Álabe 1 · cubo").is_some());
        let caption = plan_caption(&outcome, camber.as_ref(), &model.fingerprint_text());
        assert!(
            caption.contains("Álabe 3D libre (superficie media)"),
            "{caption}"
        );
        assert!(
            caption.contains("(cubo)") && caption.contains("(tapa)"),
            "{caption}"
        );
        let ruled = shaped(BladeShape::Ruled3D);
        let (model, camber) = plan_of(&ruled).unwrap();
        assert!(
            plan_caption(&ruled, camber.as_ref(), &model.fingerprint_text())
                .contains("Álabe 3D reglado")
        );
    }

    // Review Focus 3: una línea media que falla dice por qué no hay álabes.
    #[test]
    fn a_failing_camber_says_why_no_blades_are_drawn() {
        let mut outcome = shaped(BladeShape::Free3D);
        outcome.inputs.blade.bows = CamberBows {
            hub: (-30.0, -30.0),
            shroud: (-30.0, -30.0),
        };
        let message = plan_of(&outcome).unwrap_err();
        assert!(message.starts_with("Sin álabe 3D: "), "{message}");
        assert!(message.contains("línea de corriente"), "{message}");
    }

    // Review Focus 1: de vuelta a 2D, con manijas 3D puestas, la planta es la de hoy.
    #[test]
    fn back_to_2d_with_handles_the_plan_is_todays() {
        let mut inputs = Inputs::default();
        inputs.blade.bows = CamberBows {
            hub: (4.0, -2.0),
            shroud: (1.0, 3.0),
        };
        let outcome = compute(&inputs, &Basis::default()).unwrap();
        let (model, camber) = plan_of(&outcome).unwrap();
        assert!(camber.is_none());
        let today =
            hm_design::plan::plan_model(&outcome.result, outcome.geometry.as_ref().unwrap())
                .unwrap();
        assert_eq!(model, today);
        let caption = plan_caption(&outcome, None, &model.fingerprint_text());
        assert!(caption.contains("Álabe 2D (línea media)"), "{caption}");
    }
}
