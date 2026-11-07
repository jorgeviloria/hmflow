//! Pestaña Meridiano: el corte meridional, dibujado desde `MeridionalModel`.

use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Legend, Plot, PlotPoint, Points};

use crate::curves::line;
use hm_design::meridian::{
    Arm, ChannelStation, MeridianEdits, MeridianParams, MeridionalModel, meridional_model,
    meridional_model_with,
};
use hm_project::Meridian;

use crate::model::Outcome;

/// Color de cada curva del corte. Una curva sin color asignado se dibuja
/// igual, en gris: no desaparece.
fn meridian_color(name: &str) -> Color32 {
    match name {
        "Tapa delantera" => Color32::from_rgb(240, 160, 50),
        "Tapa trasera" => Color32::from_rgb(70, 150, 240),
        "Borde de ataque" => Color32::from_rgb(220, 70, 70),
        "Borde de salida" => Color32::from_rgb(80, 170, 90),
        _ => Color32::GRAY,
    }
}

/// Distancia en píxeles a la que un clic agarra un asidero.
const GRAB_PIXELS: f32 = 12.0;

/// Edita los brazos en `boxes` (las cajas de `[meridian]`) con texto o
/// arrastrando los puntos de control. Devuelve si cambió alguno.
pub(super) fn meridian_view(ui: &mut egui::Ui, outcome: &Outcome, boxes: &mut Meridian) -> bool {
    let auto = meridional_model(&outcome.result);
    let mut changed = arm_boxes(ui, boxes, auto.as_ref().ok());
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
    let p = &model.params;
    ui.label(
        RichText::new(format!(
            "Corte meridional, mm: z axial en el sentido del flujo (0 en el borde de ataque), r radial. \
             zE = {:.1}, RDs = {:.1}, g1 = {:.1}, d1i = {:.1}, anchura de entrada = {:.1}. \
             Gülich §7.2.2.1.{} Arrastra los puntos de control; cada uno se desliza por su recta. \
             El álabe sigue siendo 2D hasta F4. Huella {}.",
            p.z_e,
            p.r_ds,
            p.g1,
            p.d1i,
            p.inlet_width,
            edited_note(&edits),
            model.fingerprint_text()
        ))
        .small()
        .weak(),
    );
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
                for name in ["Tapa delantera", "Tapa trasera"] {
                    if let Some(curve) = engine.curve(name) {
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
            let handles = model.handles();
            for handle in &handles {
                plot_ui.line(
                    line(
                        "Puntos de control",
                        vec![handle.origin, handle.point],
                        Color32::DARK_GRAY,
                    )
                    .width(1.0),
                );
            }
            plot_ui.points(
                Points::new(
                    "Puntos de control",
                    handles
                        .iter()
                        .map(|handle| handle.point)
                        .collect::<Vec<_>>(),
                )
                .radius(5.0)
                .color(Color32::DARK_GRAY),
            );
            changed |= drag_handles(plot_ui, &model, boxes);
        });
    channel_graphs(ui, &model, outcome.result.qth.si);
    changed
}

/// Las cuatro cajas, con el valor automático como pista. Vacía es automático.
fn arm_boxes(ui: &mut egui::Ui, boxes: &mut Meridian, auto: Option<&MeridionalModel>) -> bool {
    let mut changed = false;
    let parsed = hm_engine::meridian_edits_of(boxes);
    ui.horizontal_wrapped(|ui| {
        for arm in Arm::ALL {
            let hint = auto
                .map(|model| format!("{} (auto)", arm_text(arm_of(&model.params, arm))))
                .unwrap_or_default();
            let text = box_mut(boxes, arm);
            let bad = !text.trim().is_empty() && edit_of(&parsed, arm).is_none();
            ui.label(RichText::new(arm.label()).small());
            let mut edit = egui::TextEdit::singleline(text)
                .id_salt(arm.file_name())
                .hint_text(hint)
                .desired_width(150.0);
            if bad {
                edit = edit.text_color(ui.visuals().error_fg_color);
            }
            changed |= ui
                .add(edit)
                .on_hover_text("Longitud con unidad, como 14 mm. Vacía: el motor.")
                .changed();
        }
        let any = boxes.arms().iter().any(|(_, text)| !text.trim().is_empty());
        if ui
            .add_enabled(any, egui::Button::new("Todo automático"))
            .clicked()
        {
            *boxes = Meridian::default();
            changed = true;
        }
    });
    changed
}

/// Agarra el asidero más cercano al empezar a arrastrar y, mientras dura,
/// escribe en su caja el brazo que da el puntero sobre su recta.
fn drag_handles(
    plot_ui: &mut egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    boxes: &mut Meridian,
) -> bool {
    let id = egui::Id::unique("meridian_dragged_arm");
    let response = plot_ui.response().clone();
    let Some(pointer) = plot_ui.pointer_coordinate() else {
        return false;
    };
    if response.drag_started() {
        let screen = plot_ui.screen_from_plot(pointer);
        let handles: Vec<(Arm, [f32; 2])> = model
            .handles()
            .iter()
            .map(|handle| {
                let at = plot_ui.screen_from_plot(PlotPoint::new(handle.point[0], handle.point[1]));
                (handle.arm, [at.x, at.y])
            })
            .collect();
        let grabbed = nearest(&handles, [screen.x, screen.y], GRAB_PIXELS);
        plot_ui.ctx().data_mut(|data| data.insert_temp(id, grabbed));
    }
    let grabbed: Option<Arm> = plot_ui.ctx().data(|data| data.get_temp(id)).flatten();
    if response.drag_stopped() {
        plot_ui
            .ctx()
            .data_mut(|data| data.remove::<Option<Arm>>(id));
    }
    let (Some(arm), true) = (grabbed, response.dragged()) else {
        return false;
    };
    let Some(value) = arm.measure([pointer.x, pointer.y], model) else {
        return false;
    };
    let text = arm_text(value.max(0.01));
    let target = box_mut(boxes, arm);
    if *target == text {
        return false;
    }
    *target = text;
    true
}

/// El asidero más cercano a `pointer`, si está a menos de `tolerance`.
fn nearest(handles: &[(Arm, [f32; 2])], pointer: [f32; 2], tolerance: f32) -> Option<Arm> {
    handles
        .iter()
        .map(|(arm, at)| (*arm, (at[0] - pointer[0]).hypot(at[1] - pointer[1])))
        .filter(|(_, distance)| *distance <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(arm, _)| arm)
}

/// Un brazo como se escribe en su caja.
fn arm_text(value: f64) -> String {
    format!("{value:.2} mm")
}

fn box_mut(boxes: &mut Meridian, arm: Arm) -> &mut String {
    match arm {
        Arm::FrontAxial => &mut boxes.front_axial_arm,
        Arm::FrontRadial => &mut boxes.front_radial_arm,
        Arm::RearAxial => &mut boxes.rear_axial_arm,
        Arm::RearRadial => &mut boxes.rear_radial_arm,
    }
}

fn edit_of(edits: &MeridianEdits, arm: Arm) -> Option<f64> {
    match arm {
        Arm::FrontAxial => edits.front_axial_arm,
        Arm::FrontRadial => edits.front_radial_arm,
        Arm::RearAxial => edits.rear_axial_arm,
        Arm::RearRadial => edits.rear_radial_arm,
    }
}

fn arm_of(params: &MeridianParams, arm: Arm) -> f64 {
    match arm {
        Arm::FrontAxial => params.front_axial_arm,
        Arm::FrontRadial => params.front_radial_arm,
        Arm::RearAxial => params.rear_axial_arm,
        Arm::RearRadial => params.rear_radial_arm,
    }
}

/// Qué brazos vienen del archivo, para la cabecera.
fn edited_note(edits: &MeridianEdits) -> String {
    let names: Vec<&str> = [
        (edits.front_axial_arm, "axial delantero"),
        (edits.front_radial_arm, "radial delantero"),
        (edits.rear_axial_arm, "axial trasero"),
        (edits.rear_radial_arm, "radial trasero"),
    ]
    .into_iter()
    .filter_map(|(arm, name)| arm.map(|_| name))
    .collect();
    if names.is_empty() {
        String::new()
    } else {
        format!(" Brazos editados: {}.", names.join(", "))
    }
}

/// A(m) y cm(m) sobre la línea media: geometría del corte, sin bloqueo de
/// álabes. No predicen rendimiento.
fn channel_graphs(ui: &mut egui::Ui, model: &MeridionalModel, qth: f64) {
    let channel = model.channel();
    let (area, velocity) = profiles(&channel, qth);
    ui.label(
        RichText::new(
            "Sobre la línea media, m en mm desde el borde de ataque. A = 2π·r·b y cm = Qth/A, sin \
             bloqueo de álabes: es geometría del corte, no una predicción de rendimiento.",
        )
        .small()
        .weak(),
    );
    ui.columns(2, |columns| {
        Plot::new("channel_area")
            .legend(Legend::default())
            .show(&mut columns[0], |plot_ui| {
                plot_ui.line(line("A, cm²", area, Color32::from_rgb(70, 150, 240)));
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

    /// Lo que dibuja la pestaña es el modelo del motor: mismas curvas, misma
    /// huella que exportará la CLI.
    #[test]
    fn the_tab_draws_the_model_of_the_shown_design() {
        let outcome = crate::model::compute(
            &crate::model::Inputs::default(),
            &crate::model::Basis::default(),
        )
        .unwrap();
        let model = hm_design::meridian::meridional_model(&outcome.result).unwrap();
        assert_eq!(model.curves.len(), 4);
    }

    #[test]
    fn the_graphs_are_the_channel_in_cm2_and_m_per_s() {
        let station = ChannelStation {
            m: 12.0,
            r: 50.0,
            width: 20.0,
            area: 2.0 * std::f64::consts::PI * 50.0 * 20.0,
        };
        let (area, velocity) = profiles(std::slice::from_ref(&station), 0.01);
        assert_eq!(area, vec![[12.0, station.area / 100.0]]);
        assert_eq!(velocity, vec![[12.0, 0.01 / (station.area * 1e-6)]]);
    }

    #[test]
    fn the_header_names_the_edited_arms() {
        assert_eq!(edited_note(&MeridianEdits::default()), "");
        let edits = MeridianEdits {
            front_radial_arm: Some(20.0),
            rear_axial_arm: Some(30.0),
            ..MeridianEdits::default()
        };
        assert_eq!(
            edited_note(&edits),
            " Brazos editados: radial delantero, axial trasero."
        );
    }

    #[test]
    fn a_click_grabs_the_nearest_handle_within_reach() {
        let handles = [
            (Arm::FrontAxial, [100.0, 100.0]),
            (Arm::FrontRadial, [110.0, 100.0]),
            (Arm::RearAxial, [300.0, 300.0]),
        ];
        assert_eq!(
            nearest(&handles, [108.0, 101.0], 12.0),
            Some(Arm::FrontRadial)
        );
        assert_eq!(nearest(&handles, [200.0, 200.0], 12.0), None);
        assert_eq!(nearest(&[], [0.0, 0.0], 12.0), None);
    }

    #[test]
    fn an_arm_is_written_in_mm_with_two_decimals() {
        assert_eq!(arm_text(14.286), "14.29 mm");
        let mut boxes = Meridian::default();
        *box_mut(&mut boxes, Arm::RearRadial) = arm_text(3.0);
        assert_eq!(boxes.rear_radial_arm, "3.00 mm");
        assert_eq!(
            hm_engine::meridian_edits_of(&boxes).rear_radial_arm,
            Some(3.0)
        );
    }
}
