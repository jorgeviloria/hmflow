//! Ventana de HidroMac Flow con egui: abre, edita el punto, guarda y exporta.
//!
//! Vive en su propio workspace, con su job `ui-egui` en el CI.
//! Ejecutar con `cargo run --manifest-path crates/hm-ui-egui/Cargo.toml`.

mod model;
mod session;

use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Corner, HLine, Legend, Line, Plot, PlotPoints, Points, Polygon, VLine};
use hm_core::Severity;
use hm_engine::{METHOD, PARAM_SPECS, PREFERRED_OPERATING_REGION};
use hm_formula::Registry;

use model::{
    CASING_BOXES, CasingBoxes, Field, FieldErrors, IntakeMode, LEAKAGE_BOXES, Outcome,
    SELECTION_BOXES, SUCTION_BOXES, build_project, compute, suction_box_error,
    suction_group_errors,
};
use session::{Command, Gate, Session};

const OPEN_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
const NEW_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N);
const SAVE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
const EXPORT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::E);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HidroMac Flow")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "HidroMac Flow",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Summary,
    Curves,
    Geometry,
}

struct App {
    session: Session,
    outcome: Option<Outcome>,
    errors: FieldErrors,
    selected: &'static str,
    tab: Tab,
    registry: Registry,
    /// Captura de pantalla para revisar la interfaz sin abrirla a mano.
    capture: Option<Capture>,
    /// Pulsaron «Reelegir» en este fotograma.
    reselect_requested: bool,
}

/// `HMF_SCREENSHOT=ruta.ppm` (y opcionalmente `HMF_TAB=curves|geometry`)
/// guarda la ventana en PPM tras unos fotogramas y cierra la aplicación.
struct Capture {
    path: std::path::PathBuf,
    frames: u32,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            session: Session::default(),
            outcome: None,
            errors: FieldErrors::new(),
            selected: "d2",
            tab: match std::env::var("HMF_TAB").as_deref() {
                Ok("curves") => Tab::Curves,
                Ok("geometry") => Tab::Geometry,
                _ => Tab::Summary,
            },
            registry: Registry::all(),
            capture: std::env::var_os("HMF_SCREENSHOT").map(|path| Capture {
                path: path.into(),
                frames: 0,
                done: Default::default(),
            }),
            reselect_requested: false,
        };
        app.recompute();
        app
    }

    /// El diseño tarda milisegundos: se recalcula con cada cambio.
    fn recompute(&mut self) {
        match compute(&self.session.inputs, &self.session.basis) {
            Ok(outcome) => {
                self.outcome = Some(outcome);
                self.errors.clear();
            }
            Err(errors) => self.errors = errors,
        }
    }

    /// Los resultados mostrados no corresponden a las entradas actuales.
    fn stale(&self) -> bool {
        self.outcome
            .as_ref()
            .is_some_and(|outcome| outcome.inputs != self.session.inputs)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(capture) = &mut self.capture {
            capture_step(ui.ctx(), capture);
        }
        top_bar(ui, self);
        // Después de la barra: un archivo recién abierto cambia el título ya.
        if let Some(title) = self.session.title_change() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        inputs_panel(ui, self);
        inspector_panel(ui, self);
        diagnostics_panel(ui, self);
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Summary, "Resumen");
                ui.selectable_value(&mut self.tab, Tab::Curves, "Curvas");
                ui.selectable_value(&mut self.tab, Tab::Geometry, "Geometría");
            });
            ui.separator();
            let Some(outcome) = &self.outcome else {
                ui.label("Sin diseño todavía: corrige las entradas.");
                return;
            };
            match self.tab {
                Tab::Summary => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        summary(ui, outcome, &mut self.selected);
                    });
                }
                Tab::Curves => curves(ui, outcome),
                Tab::Geometry => plan_view(ui, outcome),
            }
        });
    }
}

// ---------------------------------------------------------------- barra

fn top_bar(ui: &mut egui::Ui, app: &mut App) {
    let mut new_design = new_requested(ui.ctx());
    let mut open = open_requested(ui.ctx());
    let mut save_file = save_requested(ui.ctx());
    let mut save_as = false;
    let mut export_file = export_requested(ui.ctx());
    egui::Panel::top("top").show(ui, |ui| {
        let menu = file_menu(ui);
        new_design |= menu.new_design;
        open |= menu.open;
        save_file |= menu.save;
        save_as |= menu.save_as;
        export_file |= menu.export_report;
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
}

/// Archivo → Nuevo, Abrir…, Guardar, Guardar como… y Exportar informe….
fn file_menu(ui: &mut egui::Ui) -> MenuChoice {
    let mut choice = MenuChoice {
        new_design: false,
        open: false,
        save: false,
        save_as: false,
        export_report: false,
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
    let _ = app.session.export_to(&path, &json);
}

fn status(ui: &egui::Ui, app: &App) -> (&'static str, Color32) {
    let visuals = ui.visuals();
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
            ("SIN AVISOS", Color32::from_rgb(80, 180, 120))
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
        ("SIN AVISOS", Color32::from_rgb(80, 180, 120))
    }
}

// ---------------------------------------------------------------- entradas

fn inputs_panel(ui: &mut egui::Ui, app: &mut App) {
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
        (Field::Q, "Caudal Q", "120 m3/h"),
        (Field::H, "Altura H", "45 m"),
        (Field::N, "Velocidad n", "2900 rpm"),
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

// ---------------------------------------------------------------- resumen

/// Fila del resumen: clave para el inspector, etiqueta, valor y fórmula.
struct Row {
    key: &'static str,
    label: &'static str,
    value: String,
    formula: Option<&'static str>,
}

fn format_si(value: f64, unit: &str) -> String {
    match unit {
        "m" if value.abs() < 1.0 => format!("{:.2} mm", value * 1000.0),
        "m" => format!("{value:.2} m"),
        "m2" => format!("{:.1} mm²", value * 1e6),
        "m3/s" => format!("{:.2} m³/h", value * 3600.0),
        "W" => format!("{:.3} kW", value / 1000.0),
        "N" => format!("{value:.0} N"),
        "N*m" => format!("{value:.2} N·m"),
        "Pa" => format!("{:.1} MPa", value / 1e6),
        "rad" => format!("{:.2}°", value.to_degrees()),
        "m/s" => format!("{value:.2} m/s"),
        "1" => format!("{value:.4}"),
        other => format!("{value:.4} {other}"),
    }
}

fn rows(outcome: &Outcome) -> Vec<(&'static str, Vec<Row>)> {
    let result = &outcome.result;
    let selection = &result.selection;
    let mut design = vec![
        Row {
            key: "psi",
            label: "ψ",
            value: format!("{:.3}", selection.psi),
            formula: Some("design.head_coefficient"),
        },
        Row {
            key: "phi",
            label: "φ",
            value: format!("{:.3}", selection.phi),
            formula: Some("design.flow_coefficient"),
        },
        Row {
            key: "beta2",
            label: "β2",
            value: format!("{:.1}°", selection.beta2_deg),
            formula: Some("design.euler_cu2"),
        },
        Row {
            key: "blade_count",
            label: "Z",
            value: selection.blade_count.to_string(),
            formula: None,
        },
    ];
    for spec in PARAM_SPECS.iter().filter(|spec| spec.formula.is_some()) {
        if let Some(value) = hm_design::designed_si(result, spec.name) {
            design.push(Row {
                key: spec.name,
                label: spec.name,
                value: format_si(value, spec.unit),
                formula: spec.formula,
            });
        }
    }

    let hydraulics = vec![
        Row {
            key: "beta1b",
            label: "β1B",
            value: format_si(result.beta1b, "rad"),
            formula: Some("design.blade_inlet_angle"),
        },
        Row {
            key: "transition",
            label: "Subida NPSH3 Qsa/Qd",
            value: format!("{:.2}", result.transition_ratio),
            formula: Some("design.volute_matching_flow_cm"),
        },
        Row {
            key: "recirculation",
            label: "Recirculación q*RB",
            value: format!("{:.2}", result.recirculation_onset),
            formula: Some("design.recirculation_onset"),
        },
    ];

    let mut mechanics = Vec::new();
    if let Ok(mech) = &outcome.mechanics {
        let double_volute = mech.volute_wrap_deg.is_some();
        let items: [(&'static str, &'static str, String, Option<&'static str>); 10] = [
            (
                "mech.shaft",
                "Eje mínimo (torsión)",
                format_si(mech.shaft_min_diameter, "m"),
                Some("mech.shaft_min_diameter"),
            ),
            (
                "mech.hub",
                "Cubo",
                format_si(mech.hub_diameter, "m"),
                Some("mech.hub_diameter"),
            ),
            (
                "mech.blade",
                "Espesor del álabe",
                format_si(mech.blade_thickness, "m"),
                Some("mech.blade_thickness_ratio"),
            ),
            (
                "mech.shroud",
                "Espesor de las tapas",
                format_si(mech.shroud_thickness, "m"),
                Some("mech.shroud_thickness"),
            ),
            (
                "mech.clearance",
                "Holgura del anillo Δd",
                format_si(mech.seal_clearance, "m"),
                Some("mech.seal_clearance"),
            ),
            (
                "mech.radial0",
                if double_volute {
                    "Empuje radial, válvula cerrada (voluta doble)"
                } else {
                    "Empuje radial, válvula cerrada (voluta simple)"
                },
                format_si(mech.radial_thrust_shutoff, "N"),
                Some("mech.radial_thrust"),
            ),
            (
                "mech.radial1",
                if double_volute {
                    "Empuje radial, diseño (voluta doble)"
                } else {
                    "Empuje radial, diseño (voluta simple)"
                },
                format_si(mech.radial_thrust_design, "N"),
                Some("mech.radial_thrust"),
            ),
            (
                "mech.axial",
                "Empuje axial",
                format!("{:.0}..{:.0} N", mech.axial_thrust.0, mech.axial_thrust.1),
                Some(if model::balanced_axial_thrust(result) {
                    "mech.axial_thrust_balanced"
                } else {
                    "mech.axial_thrust_estimate"
                }),
            ),
            (
                "mech.driver",
                "Potencia mínima del motor",
                format!(
                    "{} × {:.2}",
                    format_si(mech.rated_power, "W"),
                    mech.driver_margin
                ),
                Some("mech.driver_power"),
            ),
            (
                "mech.motor",
                "Motor IEC",
                mech.motor_rating
                    .map_or("fuera de la serie".into(), |watts| format_si(watts, "W")),
                Some("mech.driver_power"),
            ),
        ];
        mechanics.extend(items.into_iter().map(|(key, label, value, formula)| Row {
            key,
            label,
            value,
            formula,
        }));
    }

    let casing = model::casing_rows(outcome)
        .into_iter()
        .map(|row| Row {
            key: row.key,
            label: row.label,
            value: row.value,
            formula: row.formula,
        })
        .collect();

    vec![
        ("Diseño", design),
        ("Hidráulica fina", hydraulics),
        ("Carcasa", casing),
        ("Mecánica", mechanics),
    ]
}

fn summary(ui: &mut egui::Ui, outcome: &Outcome, selected: &mut &'static str) {
    ui.label(
        RichText::new("Pulsa un valor para ver su fórmula, fuente y supuestos.")
            .small()
            .weak(),
    );
    for (title, group) in rows(outcome) {
        if group.is_empty() {
            continue;
        }
        // Los valores de diseño son cortos; los de mecánica llevan etiquetas
        // largas y van en dos columnas.
        let columns = if title == "Diseño" { 4 } else { 2 };
        egui::CollapsingHeader::new(RichText::new(title).strong())
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new(title)
                    .num_columns(columns)
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for (index, row) in group.iter().enumerate() {
                            let text = format!("{}  {}", row.label, row.value);
                            if ui.selectable_label(*selected == row.key, text).clicked() {
                                *selected = row.key;
                            }
                            if index % columns == columns - 1 {
                                ui.end_row();
                            }
                        }
                    });
            });
    }
}

// ---------------------------------------------------------------- inspector

fn inspector_panel(ui: &mut egui::Ui, app: &App) {
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

// ---------------------------------------------------------------- avisos

fn diagnostics_panel(ui: &mut egui::Ui, app: &App) {
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

// ---------------------------------------------------------------- curvas

fn line(name: &str, points: Vec<[f64; 2]>, color: Color32) -> Line<'static> {
    Line::new(name.to_string(), PlotPoints::from(points))
        .color(color)
        .width(2.0)
}

fn curves(ui: &mut egui::Ui, outcome: &Outcome) {
    let q_design = outcome.input.q.si * 3600.0;
    let (por_low, por_high) = PREFERRED_OPERATING_REGION;
    let curve = &outcome.curve;
    let q = |index: usize| curve[index].q.si * 3600.0;
    let head: Vec<[f64; 2]> = (0..curve.len()).map(|i| [q(i), curve[i].h.si]).collect();
    let npshr: Vec<[f64; 2]> = (0..curve.len())
        .map(|i| [q(i), curve[i].npshr.si])
        .collect();
    let eta: Vec<[f64; 2]> = (0..curve.len())
        .map(|i| [q(i), curve[i].eta * 100.0])
        .collect();
    let power: Vec<[f64; 2]> = (0..curve.len())
        .map(|i| [q(i), curve[i].p_shaft.si / 1000.0])
        .collect();
    let h_top = head.iter().map(|p| p[1]).fold(0.0, f64::max) * 1.1;

    let blue = Color32::from_rgb(70, 150, 240);
    let orange = Color32::from_rgb(240, 160, 50);
    let green = Color32::from_rgb(80, 180, 120);
    let purple = Color32::from_rgb(180, 120, 220);
    let qd_color = ui.visuals().strong_text_color();
    // Las marcas comunes solo llevan nombre (y entrada en la leyenda) en la
    // primera gráfica.
    let marks = |plot_ui: &mut egui_plot::PlotUi, top: f64, named: bool| {
        let name = |text: &str| {
            if named {
                text.to_string()
            } else {
                String::new()
            }
        };
        plot_ui.polygon(
            Polygon::new(
                name("Región preferente 0,7..1,2·Qd"),
                PlotPoints::from(vec![
                    [por_low * q_design, 0.0],
                    [por_high * q_design, 0.0],
                    [por_high * q_design, top],
                    [por_low * q_design, top],
                ]),
            )
            .fill_color(Color32::from_rgba_unmultiplied(80, 180, 120, 28))
            .stroke(egui::Stroke::new(0.0, green)),
        );
        plot_ui.vline(VLine::new(name("Qd"), q_design).color(qd_color).width(1.5));
        plot_ui.vline(
            VLine::new(
                name("Inicio de recirculación"),
                outcome.result.recirculation_onset * q_design,
            )
            .color(purple)
            .style(egui_plot::LineStyle::dashed_dense()),
        );
    };

    let height = (ui.available_height() - 20.0) / 3.0;
    let group = ui.scope_id().with("q-axis");

    Plot::new("head")
        .height(height.max(160.0))
        .legend(Legend::default().position(Corner::RightTop))
        .y_axis_label("m")
        .link_axis(group, [true, false])
        .link_cursor(group, [true, false])
        .include_y(0.0)
        .show(ui, |plot_ui| {
            marks(plot_ui, h_top, true);
            plot_ui.line(line("H", head, blue));
            plot_ui.line(line("NPSHr", npshr, orange));
            if let Some(npsha) = &outcome.input.npsha {
                plot_ui.hline(HLine::new("NPSHa", npsha.si).color(green));
            }
            plot_ui.points(
                Points::new("Punto de diseño", vec![[q_design, outcome.input.h.si]])
                    .radius(5.0)
                    .color(blue),
            );
        });

    Plot::new("efficiency")
        .height(height.max(120.0))
        .legend(Legend::default().position(Corner::RightBottom))
        .y_axis_label("η %")
        .link_axis(group, [true, false])
        .link_cursor(group, [true, false])
        .include_y(0.0)
        .show(ui, |plot_ui| {
            marks(plot_ui, 100.0, false);
            plot_ui.line(line("η", eta, green));
        });

    let p_top = power.iter().map(|p| p[1]).fold(0.0, f64::max) * 1.2;
    Plot::new("power")
        .height(height.max(120.0))
        .legend(Legend::default().position(Corner::RightBottom))
        .x_axis_label("Q m³/h")
        .y_axis_label("kW")
        .link_axis(group, [true, false])
        .link_cursor(group, [true, false])
        .include_y(0.0)
        .show(ui, |plot_ui| {
            marks(plot_ui, p_top, false);
            plot_ui.line(line("P eje", power, orange));
            if let Some(rating) = outcome
                .mechanics
                .as_ref()
                .ok()
                .and_then(|mech| mech.motor_rating)
            {
                plot_ui.hline(HLine::new("Motor IEC", rating / 1000.0).color(Color32::RED));
            }
        });
}

// ---------------------------------------------------------------- geometría

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

fn plan_view(ui: &mut egui::Ui, outcome: &Outcome) {
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

fn capture_step(ctx: &egui::Context, capture: &mut Capture) {
    capture.frames += 1;
    ctx.request_repaint();
    if capture.frames == 5 {
        let path = capture.path.clone();
        let done = capture.done.clone();
        let callback = egui::ScreenshotCallback::new(move |image| {
            let [width, height] = image.size;
            let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
            for pixel in &image.pixels {
                bytes.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
            }
            if let Err(error) = std::fs::write(&path, bytes) {
                eprintln!("no se pudo guardar la captura: {error}");
            }
            done.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
    }
    if capture.done.load(std::sync::atomic::Ordering::SeqCst) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
