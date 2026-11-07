//! Inspector: la fórmula, la fuente y los supuestos del valor elegido.

use eframe::egui;
use egui::RichText;

use crate::App;
use crate::summary::rows;

pub(super) fn inspector_panel(ui: &mut egui::Ui, app: &App) {
    egui::Panel::right("inspector")
        .resizable(true)
        .default_size(330.0)
        .show(ui, |ui| {
            ui.heading("Inspector");
            let Some(outcome) = &app.outcome else {
                return;
            };
            let all = rows(outcome);
            let Some(row) = all
                .iter()
                .flat_map(|(_, group)| group.iter())
                .find(|row| row.key == app.selected)
            else {
                ui.label("Elige un valor en el resumen.");
                return;
            };
            ui.label(RichText::new(row.label).weak());
            ui.label(RichText::new(&row.value).size(26.0).strong());
            let Some(id) = row.formula else {
                ui.label(if row.key.starts_with("casing.") {
                    "Comparación con la cota de [casing]; el aviso, si lo hay, dice qué hacer."
                } else {
                    "Selección por tabla de nq; sin fórmula."
                });
                return;
            };
            let Some(spec) = app.registry.get(id) else {
                return;
            };
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(RichText::new(&spec.title).strong());
                ui.label(RichText::new(id).monospace().weak());
                if let Some(expression) = spec.expr.get("value") {
                    ui.add_space(4.0);
                    ui.label(RichText::new(expression).monospace());
                }
                ui.add_space(6.0);
                egui::Grid::new("formula").num_columns(2).show(ui, |ui| {
                    ui.label(RichText::new("Validación").weak());
                    ui.label(format!("{:?}", spec.validation));
                    ui.end_row();
                    ui.label(RichText::new("Unidades").weak());
                    ui.label(&spec.units);
                    ui.end_row();
                    ui.label(RichText::new("Rango").weak());
                    ui.label(&spec.range);
                    ui.end_row();
                    for (name, unit) in &spec.inputs {
                        ui.label(RichText::new("Entrada").weak());
                        ui.label(format!("{name} [{unit}]"));
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                ui.label(RichText::new("Fuente").weak());
                ui.label(&spec.source);
                if let Some(assumptions) = spec
                    .notes
                    .as_ref()
                    .and_then(|notes| notes.assumptions.as_ref())
                {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Supuestos").weak());
                    ui.label(assumptions);
                }
            });
        });
}
