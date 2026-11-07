//! Pestaña Meridiano: el corte meridional, dibujado desde `MeridionalModel`.

use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Legend, Plot};

use crate::curves::line;
use hm_design::meridian::{ChannelStation, MeridianEdits, MeridionalModel};

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

/// `edits`: los brazos de `[meridian]` del proyecto abierto.
pub(super) fn meridian_view(ui: &mut egui::Ui, outcome: &Outcome, edits: &MeridianEdits) {
    // El visor no calcula: dibuja el mismo modelo que se exporta.
    let model = match hm_design::meridian::meridional_model_with(&outcome.result, edits) {
        Ok(model) => model,
        Err(error) => {
            ui.label(format!("Sin meridiano: {error}."));
            return;
        }
    };
    let p = &model.params;
    ui.label(
        RichText::new(format!(
            "Corte meridional, mm: z axial en el sentido del flujo (0 en el borde de ataque), r radial. \
             zE = {:.1}, RDs = {:.1}, g1 = {:.1}, d1i = {:.1}, anchura de entrada = {:.1}. \
             Gülich §7.2.2.1.{} El álabe sigue siendo 2D hasta F4. Huella {}.",
            p.z_e,
            p.r_ds,
            p.g1,
            p.d1i,
            p.inlet_width,
            edited_note(edits),
            model.fingerprint_text()
        ))
        .small()
        .weak(),
    );
    let graphs_height = (ui.available_height() * 0.35).max(140.0);
    Plot::new("meridian")
        .data_aspect(1.0)
        .legend(Legend::default())
        .height(ui.available_height() - graphs_height)
        .show(ui, |plot_ui| {
            for curve in &model.curves {
                let color = meridian_color(&curve.name);
                plot_ui.line(line(&curve.name, curve.points.clone(), color));
            }
        });
    channel_graphs(ui, &model, outcome.result.qth.si);
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
        format!(" Brazos editados en [meridian]: {}.", names.join(", "))
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
            " Brazos editados en [meridian]: radial delantero, axial trasero."
        );
    }
}
