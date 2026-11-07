//! Panel de avisos del diseño.

use eframe::egui;
use egui::RichText;
use hm_core::Severity;

use crate::App;
use crate::model;

pub(super) fn diagnostics_panel(ui: &mut egui::Ui, app: &App) {
    egui::Panel::bottom("diagnostics")
        .resizable(true)
        .default_size(150.0)
        .show(ui, |ui| {
            if let Some(note) = app.session.migration_note() {
                ui.label(RichText::new(note).color(ui.visuals().warn_fg_color));
            }
            if let Some(notice) = app.session.panel_notice() {
                ui.label(RichText::new(notice).color(ui.visuals().error_fg_color));
            }
            let Some(outcome) = &app.outcome else {
                return;
            };
            if outcome.reselect_hint {
                ui.label(RichText::new(model::RESELECT_HINT).color(ui.visuals().warn_fg_color));
            }
            // Los avisos del archivo sustituyen a los de `compute` hasta que
            // cambia una caja. Sumarlos repetiría los del método actual.
            let from_file = app.session.opened_diagnostics.as_ref();
            let diagnostics = from_file.unwrap_or(&outcome.diagnostics);
            let count = |severity: Severity| {
                diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.severity == severity)
                    .count()
            };
            ui.horizontal(|ui| {
                ui.strong("Avisos");
                ui.label(format!(
                    "{} errores · {} avisos · {} informativos",
                    count(Severity::Error) + count(Severity::Blocking),
                    count(Severity::Warning),
                    count(Severity::Info)
                ));
            });
            egui::ScrollArea::vertical().show(ui, |ui| {
                if from_file.is_none() {
                    for (name, error) in [
                        ("geometría", outcome.geometry.as_ref().err()),
                        ("mecánica", outcome.mechanics.as_ref().err()),
                        ("viscosidad", outcome.viscous.as_ref().err()),
                    ] {
                        if let Some(error) = error {
                            ui.label(
                                RichText::new(format!("{name} no disponible: {error}"))
                                    .color(ui.visuals().warn_fg_color),
                            );
                        }
                    }
                }
                for diagnostic in diagnostics {
                    let color = match diagnostic.severity {
                        Severity::Error | Severity::Blocking => ui.visuals().error_fg_color,
                        Severity::Warning => ui.visuals().warn_fg_color,
                        Severity::Info => ui.visuals().weak_text_color(),
                    };
                    ui.label(
                        RichText::new(format!("{}  {}", diagnostic.code, diagnostic.message))
                            .color(color),
                    );
                    if let Some(hint) = &diagnostic.hint {
                        ui.label(RichText::new(format!("    → {hint}")).small().weak());
                    }
                }
            });
        });
}
