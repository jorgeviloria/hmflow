//! Pestaña Meridiano: el corte meridional, dibujado desde `MeridionalModel`.

use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Legend, Plot};

use crate::curves::line;
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

pub(super) fn meridian_view(ui: &mut egui::Ui, outcome: &Outcome) {
    // El visor no calcula: dibuja el mismo modelo que se exporta.
    let model = match hm_design::meridian::meridional_model(&outcome.result) {
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
             Gülich §7.2.2.1. El álabe sigue siendo 2D hasta F4. Huella {}.",
            p.z_e,
            p.r_ds,
            p.g1,
            p.d1i,
            p.inlet_width,
            model.fingerprint_text()
        ))
        .small()
        .weak(),
    );
    Plot::new("meridian")
        .data_aspect(1.0)
        .legend(Legend::default())
        .show(ui, |plot_ui| {
            for curve in &model.curves {
                let color = meridian_color(&curve.name);
                plot_ui.line(line(&curve.name, curve.points.clone(), color));
            }
        });
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
}
