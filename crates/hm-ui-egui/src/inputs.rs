//! Panel de entradas: punto de diseño, carcasa, selecciones y medidas.

use eframe::egui;
use egui::RichText;

use crate::App;
use crate::model;
use crate::model::{
    CASING_BOXES, CasingBoxes, Field, FieldErrors, IntakeMode, LEAKAGE_BOXES, SELECTION_BOXES,
    SUCTION_BOXES, suction_box_error, suction_group_errors,
};

pub(super) fn inputs_panel(ui: &mut egui::Ui, app: &mut App) {
    egui::Panel::left("inputs")
        .resizable(true)
        .default_size(220.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| inputs_content(ui, app));
        });
}

fn inputs_content(ui: &mut egui::Ui, app: &mut App) {
    ui.heading("Punto de diseño");
    ui.add_space(4.0);
    let mut changed = false;
    let errors = app.errors.clone();
    let eta_m_locked = app.session.basis.eta_m_is_locked();
    for (field, label, hint) in [
        (Field::Q, "Caudal Q", "1060 gpm"),
        (Field::H, "Altura H", "83 m"),
        (Field::N, "Velocidad n", "1750 rpm"),
        (Field::Temperature, "Temperatura del agua, °C", "20"),
        (Field::EtaM, "ηm cojinetes y cierre (vacío: Gülich)", "auto"),
    ] {
        let value = match field {
            Field::Q => &mut app.session.inputs.q,
            Field::H => &mut app.session.inputs.h,
            Field::N => &mut app.session.inputs.n,
            Field::Temperature => &mut app.session.inputs.temperature_c,
            Field::EtaM => &mut app.session.inputs.eta_m,
            Field::Npsha
            | Field::Suction
            | Field::Casing(_)
            | Field::Selection(_)
            | Field::General => continue,
        };
        // ηm en `locked` manda el archivo: escribir aquí no haría nada.
        let enabled = field != Field::EtaM || !eta_m_locked;
        ui.add_enabled_ui(enabled, |ui| {
            changed |= text_box(ui, label, value, hint, error_of(&errors, field), label);
        });
    }

    ui.label("NPSH disponible");
    ui.horizontal(|ui| {
        changed |= ui
            .selectable_value(
                &mut app.session.inputs.intake,
                IntakeMode::Written,
                "escrito",
            )
            .changed();
        changed |= ui
            .selectable_value(
                &mut app.session.inputs.intake,
                IntakeMode::Installation,
                "instalación",
            )
            .changed();
    });
    match app.session.inputs.intake {
        IntakeMode::Written => {
            changed |= text_box(
                ui,
                "NPSHa (opcional)",
                &mut app.session.inputs.npsha,
                "vacío si no se conoce",
                error_of(&errors, Field::Npsha),
                "npsha",
            );
        }
        IntakeMode::Installation => {
            let boxes = [
                (
                    &mut app.session.inputs.suction_pressure,
                    "Presión en la aspiración",
                    "1 atm",
                ),
                (
                    &mut app.session.inputs.suction_height,
                    "Altura (opcional)",
                    "2 m",
                ),
                (
                    &mut app.session.inputs.suction_losses,
                    "Pérdidas (opcional)",
                    "0.5 m",
                ),
                (
                    &mut app.session.inputs.suction_vapor,
                    "Presión de vapor (opcional)",
                    "vacío: la del agua",
                ),
            ];
            for ((value, label, hint), (_, name)) in boxes.into_iter().zip(SUCTION_BOXES) {
                let error = suction_box_error(&errors, name);
                changed |= text_box(ui, label, value, hint, error, label);
            }
            for message in suction_group_errors(&errors) {
                ui.label(
                    RichText::new(message)
                        .small()
                        .color(ui.visuals().error_fg_color),
                );
            }
        }
    }
    ui.add_space(4.0);
    changed |= casing_section(ui, app, &errors);
    changed |= selections_section(ui, app, &errors);
    if app.reselect_requested {
        app.reselect_requested = false;
        // El aviso del panel dice qué cambió o por qué no se pudo.
        let _ = app.session.reselect();
        changed = false;
        app.recompute();
    }
    for (_, message) in errors.iter().filter(|(field, _)| *field == Field::General) {
        ui.label(
            RichText::new(message)
                .small()
                .color(ui.visuals().error_fg_color),
        );
    }
    if changed {
        app.session.edited();
        app.recompute();
    }
    ui.separator();
    ui.label(
        RichText::new(
            "Se recalcula al escribir. La densidad y la viscosidad salen de \
                     la temperatura (Kell; IAPWS R12-08).",
        )
        .small()
        .weak(),
    );
    if let Some(outcome) = &app.outcome {
        ui.label(
            RichText::new(format!("ρ = {:.1} kg/m³", outcome.input.rho.si))
                .small()
                .weak(),
        );
        if let Ok(Some(viscous)) = &outcome.viscous {
            ui.label(
                RichText::new(format!(
                    "ν = {:.3} cSt · fH = {:.3} · fη = {:.3}",
                    viscous.nu * 1e6,
                    viscous.f_h,
                    viscous.f_eta
                ))
                .small()
                .weak(),
            );
        }
    }
}

/// Bloque de la carcasa. Plegado sin carcasa. Devuelve si algo cambió.
fn casing_section(ui: &mut egui::Ui, app: &mut App, errors: &FieldErrors) -> bool {
    let has_casing = app.session.inputs.casing.is_some();
    let mut changed = false;
    // El id cambia con la casilla: un archivo con carcasa se abre desplegado.
    egui::CollapsingHeader::new("Carcasa")
        .id_salt(("casing", has_casing))
        .default_open(has_casing)
        .show(ui, |ui| {
            let mut wanted = has_casing;
            if ui
                .checkbox(&mut wanted, "Rodete para una carcasa existente")
                .changed()
            {
                if wanted {
                    app.session.add_casing();
                    changed = true;
                } else if app.session.remove_casing() {
                    changed = true;
                }
            }
            let Some(boxes) = &mut app.session.inputs.casing else {
                return;
            };
            changed |= casing_boxes(ui, boxes, &CASING_BOXES, errors);
            let has_leakage = LEAKAGE_BOXES.iter().any(|(name, _, _)| {
                boxes
                    .text_mut(name)
                    .is_some_and(|text| !text.trim().is_empty())
            });
            egui::CollapsingHeader::new("Anillos y agujeros (todos o ninguno)")
                .id_salt("leakage")
                .default_open(has_leakage)
                .show(ui, |ui| {
                    changed |= casing_boxes(ui, boxes, &LEAKAGE_BOXES, errors);
                });
        });
    changed
}

/// Bloque de selecciones y medidas. Plegado si no hay nada que mostrar.
fn selections_section(ui: &mut egui::Ui, app: &mut App, errors: &FieldErrors) -> bool {
    let key = app.session.view_key().map(std::path::Path::to_path_buf);
    let force = app.session.take_open_selections()
        || model::selections_needed(errors, app.outcome.as_ref());
    let inputs = &mut app.session.inputs;
    let filled = SELECTION_BOXES.iter().any(|(name, _, _)| {
        inputs
            .selection_mut(name)
            .is_some_and(|text| !text.trim().is_empty())
    });
    let open = filled || inputs.casing.is_some();
    let mut changed = false;
    let mut requested = false;
    let can_reselect = app.session.project.is_some();
    // Un archivo con algo fijado se abre desplegado.
    egui::CollapsingHeader::new("Selecciones y medidas")
        .id_salt(("selections", key))
        .default_open(open)
        .open(force.then_some(true))
        .show(ui, |ui| {
            for (name, label, hint) in SELECTION_BOXES {
                if let Some(value) = inputs.selection_mut(name) {
                    let error = error_of(errors, Field::Selection(name));
                    changed |= text_box(ui, label, value, hint, error, name);
                }
            }
            requested = ui
                .add_enabled(can_reselect, egui::Button::new("Reelegir con el motor"))
                .on_hover_text(
                    "Rehace lo que esté en manual para este punto. Lo fijado se respeta. \
                     No se escribe hasta guardar.",
                )
                .clicked();
        });
    app.reselect_requested = requested;
    changed
}

fn casing_boxes(
    ui: &mut egui::Ui,
    boxes: &mut CasingBoxes,
    list: &[(&'static str, &str, &str)],
    errors: &FieldErrors,
) -> bool {
    let mut changed = false;
    for (name, label, hint) in list {
        if let Some(value) = boxes.text_mut(name) {
            let error = error_of(errors, Field::Casing(name));
            changed |= text_box(ui, label, value, hint, error, name);
        }
    }
    changed
}

/// El mensaje de error de un campo, si lo hay.
fn error_of(errors: &FieldErrors, field: Field) -> Option<&str> {
    errors
        .iter()
        .find(|(which, _)| *which == field)
        .map(|(_, message)| message.as_str())
}

/// Etiqueta, caja de texto y su error debajo. Devuelve si el texto cambió.
///
/// `id` fija la identidad de la caja: sin él egui la saca de su posición y dos
/// cajas de modos distintos en el mismo lugar se pasarían el foco.
fn text_box(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    error: Option<&str>,
    id: &str,
) -> bool {
    ui.label(label);
    let mut edit = egui::TextEdit::singleline(value)
        .id_salt(id)
        .hint_text(hint);
    if error.is_some() {
        edit = edit.text_color(ui.visuals().error_fg_color);
    }
    let changed = ui.add(edit).changed();
    if let Some(message) = error {
        ui.label(
            RichText::new(message)
                .small()
                .color(ui.visuals().error_fg_color),
        );
    }
    ui.add_space(4.0);
    changed
}
