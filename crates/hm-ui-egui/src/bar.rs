//! Barra superior: menú Archivo, atajos, abrir, guardar, exportar y estado.

use eframe::egui;
use egui::{Color32, RichText};
use hm_core::Severity;
use hm_engine::METHOD;

use crate::App;
use crate::model::build_project;
use crate::session::{Command, Gate};

const OPEN_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
const NEW_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N);
const SAVE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
const EXPORT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::E);

// ---------------------------------------------------------------- barra

pub(super) fn top_bar(ui: &mut egui::Ui, app: &mut App) {
    let mut new_design = new_requested(ui.ctx());
    let mut open = open_requested(ui.ctx());
    let mut save_file = save_requested(ui.ctx());
    let mut save_as = false;
    let mut export_file = export_requested(ui.ctx());
    let mut export_geometry = false;
    let mut export_meridian = false;
    egui::Panel::top("top").show(ui, |ui| {
        let menu = file_menu(ui);
        new_design |= menu.new_design;
        open |= menu.open;
        save_file |= menu.save;
        save_as |= menu.save_as;
        export_file |= menu.export_report;
        export_geometry |= menu.export_geometry;
        export_meridian |= menu.export_meridian;
        ui.horizontal(|ui| {
            ui.heading("HidroMac Flow");
            if let Some(name) = app.session.bar_label() {
                ui.label(RichText::new(name).strong());
            }
            ui.label(
                RichText::new(format!("Diseño preliminar end-suction · método {}", METHOD)).weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, color) = status(ui, app);
                ui.label(RichText::new(text).strong().color(color));
            });
        });
        if let Some(notice) = app.session.discard_notice() {
            ui.label(RichText::new(notice).color(ui.visuals().error_fg_color));
        }
    });
    if new_design {
        apply_new(app);
    }
    if open {
        choose_and_open(app);
    }
    if save_as {
        choose_and_save(app);
    } else if save_file {
        save(app);
    }
    if export_file {
        choose_and_export(app);
    }
    if export_geometry {
        choose_and_export_geometry(app);
    }
    if export_meridian {
        choose_and_export_meridian(app);
    }
}

/// Ctrl+N y, en macOS, también ⌘N. `COMMAND` es ⌘ en Mac y Ctrl en el resto.
fn new_requested(ctx: &egui::Context) -> bool {
    pressed_once(ctx, NEW_SHORTCUT) || pressed_once(ctx, ctrl(NEW_SHORTCUT))
}

/// Ctrl+O y, en macOS, también ⌘O. `COMMAND` es ⌘ en Mac y Ctrl en el resto.
fn open_requested(ctx: &egui::Context) -> bool {
    pressed_once(ctx, OPEN_SHORTCUT) || pressed_once(ctx, ctrl(OPEN_SHORTCUT))
}

/// Ctrl+S y, en macOS, también ⌘S.
fn save_requested(ctx: &egui::Context) -> bool {
    pressed_once(ctx, SAVE_SHORTCUT) || pressed_once(ctx, ctrl(SAVE_SHORTCUT))
}

/// Ctrl+E y, en macOS, también ⌘E.
fn export_requested(ctx: &egui::Context) -> bool {
    pressed_once(ctx, EXPORT_SHORTCUT) || pressed_once(ctx, ctrl(EXPORT_SHORTCUT))
}

fn ctrl(shortcut: egui::KeyboardShortcut) -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, shortcut.logical_key)
}

/// El atajo, sin contar las repeticiones de una tecla mantenida.
///
/// `consume_shortcut` las incluye, y Nuevo y Abrir confirman el descarte al
/// repetirse: dejar la tecla apretada tiraría el trabajo. La repetición se
/// consume igual, para que no llegue a otro sitio.
fn pressed_once(ctx: &egui::Context, shortcut: egui::KeyboardShortcut) -> bool {
    ctx.input_mut(|input| {
        let fresh = input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    repeat: false,
                    ..
                } if *key == shortcut.logical_key
                    && modifiers.matches_logically(shortcut.modifiers)
            )
        });
        input.consume_shortcut(&shortcut) && fresh
    })
}

struct MenuChoice {
    new_design: bool,
    open: bool,
    save: bool,
    save_as: bool,
    export_report: bool,
    export_geometry: bool,
    export_meridian: bool,
}

/// Archivo → Nuevo, Abrir…, Guardar, Guardar como…, Exportar informe… y
/// Exportar geometría….
fn file_menu(ui: &mut egui::Ui) -> MenuChoice {
    let mut choice = MenuChoice {
        new_design: false,
        open: false,
        save: false,
        save_as: false,
        export_report: false,
        export_geometry: false,
        export_meridian: false,
    };
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("Archivo", |ui| {
            let new_button =
                egui::Button::new("Nuevo").shortcut_text(ui.ctx().format_shortcut(&NEW_SHORTCUT));
            if ui.add(new_button).clicked() {
                ui.close();
                choice.new_design = true;
            }
            let open_button =
                egui::Button::new("Abrir…").shortcut_text(ui.ctx().format_shortcut(&OPEN_SHORTCUT));
            if ui.add(open_button).clicked() {
                ui.close();
                choice.open = true;
            }
            ui.separator();
            let save_button = egui::Button::new("Guardar")
                .shortcut_text(ui.ctx().format_shortcut(&SAVE_SHORTCUT));
            if ui.add(save_button).clicked() {
                ui.close();
                choice.save = true;
            }
            if ui.button("Guardar como…").clicked() {
                ui.close();
                choice.save_as = true;
            }
            ui.separator();
            let export_button = egui::Button::new("Exportar informe…")
                .shortcut_text(ui.ctx().format_shortcut(&EXPORT_SHORTCUT));
            if ui.add(export_button).clicked() {
                ui.close();
                choice.export_report = true;
            }
            if ui.button("Exportar geometría…").clicked() {
                ui.close();
                choice.export_geometry = true;
            }
            if ui.button("Exportar meridiano…").clicked() {
                ui.close();
                choice.export_meridian = true;
            }
        });
        ui.menu_button("Ver", |ui| {
            ui.label("Tema");
            let mut preference = ui.ctx().options(|o| o.theme_preference);
            let before = preference;
            ui.radio_value(&mut preference, egui::ThemePreference::System, "Sistema");
            ui.radio_value(&mut preference, egui::ThemePreference::Light, "Claro");
            ui.radio_value(&mut preference, egui::ThemePreference::Dark, "Oscuro");
            if preference != before {
                ui.ctx().set_theme(preference);
            }
        });
    });
    choice
}

fn apply_new(app: &mut App) {
    if app.session.request(Command::New) == Gate::Proceed {
        app.session.new_design();
        app.recompute();
    }
}

fn choose_and_open(app: &mut App) {
    if app.session.request(Command::Open) != Gate::Proceed {
        return;
    }
    let Some(path) = rfd::FileDialog::new()
        .set_title("Abrir proyecto")
        .add_filter("HidroMac Flow", &["hmf"])
        .pick_file()
    else {
        // El diálogo no descartó nada: la próxima orden vuelve a pedir confirmación.
        if app.session.dirty {
            app.session.notice = None;
        }
        return;
    };
    if app.session.open_path(&path) {
        app.recompute();
    }
}

/// Sin ruta, Guardar es Guardar como. Con errores de caja o de etapa no abre
/// el diálogo ni escribe.
fn save(app: &mut App) {
    if !can_save(app) {
        return;
    }
    if let Some(path) = app.session.file.clone() {
        write_to(app, &path);
        return;
    }
    choose_and_save(app);
}

fn choose_and_save(app: &mut App) {
    if !can_save(app) {
        return;
    }
    app.session.disarm();
    let Some(path) = rfd::FileDialog::new()
        .set_title("Guardar proyecto")
        .add_filter("HidroMac Flow", &["hmf"])
        .save_file()
    else {
        return;
    };
    write_to(app, &path);
}

fn write_to(app: &mut App, path: &std::path::Path) {
    if app.session.save_to(path).is_ok() {
        app.recompute();
    }
}

fn can_save(app: &mut App) -> bool {
    let name = app.session.project_name();
    match build_project(
        &app.session.inputs,
        &app.session.basis,
        app.session.project.as_ref(),
        &name,
    ) {
        Ok(_) => true,
        Err(message) => {
            app.session.disarm();
            app.session.notice = Some(message);
            false
        }
    }
}

/// Con errores de caja o de etapa no abre el diálogo ni escribe. El JSON no
/// toca el `.hmf`, el proyecto ni el `●`.
fn choose_and_export(app: &mut App) {
    let Ok(json) = app.session.export_json() else {
        return;
    };
    let Some(path) = rfd::FileDialog::new()
        .set_title("Exportar informe")
        .add_filter("Informe JSON", &["json"])
        .set_file_name(app.session.project_name())
        .save_file()
    else {
        return;
    };
    // Si falla, el error ya está en `notice`.
    let _ = app.session.export_to(&path, &json, "json");
}

/// El corte meridional en CSV, el mismo que dibuja la pestaña.
fn choose_and_export_meridian(app: &mut App) {
    let Ok(csv) = app.session.export_meridian_csv() else {
        return;
    };
    let Some(path) = rfd::FileDialog::new()
        .set_title("Exportar meridiano")
        .add_filter("CSV", &["csv"])
        .set_file_name(format!("{}-meridiano", app.session.project_name()))
        .save_file()
    else {
        return;
    };
    // Si falla, el error ya está en `notice`.
    let _ = app.session.export_to(&path, &csv, "csv");
}

/// La vista en planta en CSV, la misma que dibuja el visor. Con errores de
/// caja o de etapa no abre el diálogo ni escribe.
fn choose_and_export_geometry(app: &mut App) {
    let Ok(csv) = app.session.export_csv() else {
        return;
    };
    let Some(path) = rfd::FileDialog::new()
        .set_title("Exportar geometría")
        .add_filter("CSV", &["csv"])
        .set_file_name(app.session.project_name())
        .save_file()
    else {
        return;
    };
    // Si falla, el error ya está en `notice`.
    let _ = app.session.export_to(&path, &csv, "csv");
}

fn status(ui: &egui::Ui, app: &App) -> (&'static str, Color32) {
    let visuals = ui.visuals();
    let ok = crate::theme::palette(ui.ctx()).ok;
    let Some(outcome) = &app.outcome else {
        return ("SIN DISEÑO", visuals.error_fg_color);
    };
    if !app.errors.is_empty() || app.stale() {
        return (
            "ENTRADAS INVÁLIDAS · resultados anteriores",
            visuals.error_fg_color,
        );
    }
    // Con un archivo recién abierto, la barra mira esos avisos, no los de
    // `compute`: la lista es una sola.
    if let Some(diagnostics) = &app.session.opened_diagnostics {
        let stage_failed =
            outcome.geometry.is_err() || outcome.mechanics.is_err() || outcome.viscous.is_err();
        if stage_failed || diagnostics.iter().any(hm_core::Diagnostic::is_error) {
            ("ERRORES", visuals.error_fg_color)
        } else if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Warning)
        {
            ("REVISAR AVISOS", visuals.warn_fg_color)
        } else {
            ("SIN AVISOS", ok)
        }
    } else if outcome.has_errors() {
        ("ERRORES", visuals.error_fg_color)
    } else if outcome
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Warning)
    {
        ("REVISAR AVISOS", visuals.warn_fg_color)
    } else {
        ("SIN AVISOS", ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_event(key: egui::Key, pressed: bool) -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        }
    }

    /// Cuántos fotogramas dieron la orden. `true` pulsa la tecla, `false` la
    /// suelta; dos `true` seguidos son la repetición de una tecla mantenida.
    fn orders(requested: fn(&egui::Context) -> bool, key: egui::Key, presses: &[bool]) -> usize {
        let ctx = egui::Context::default();
        let mut count = 0;
        for pressed in presses {
            let mut output = ctx.run_ui(key_event(key, *pressed), |ui| {
                count += usize::from(requested(ui.ctx()));
            });
            output.textures_delta.clear();
        }
        count
    }

    #[test]
    fn holding_a_shortcut_is_one_order() {
        let hold = [true, true, true];
        assert_eq!(orders(new_requested, egui::Key::N, &hold), 1);
        assert_eq!(orders(open_requested, egui::Key::O, &hold), 1);
    }

    #[test]
    fn pressing_a_shortcut_again_is_a_second_order() {
        let twice = [true, false, true];
        assert_eq!(orders(new_requested, egui::Key::N, &twice), 2);
        assert_eq!(orders(open_requested, egui::Key::O, &twice), 2);
    }
}
