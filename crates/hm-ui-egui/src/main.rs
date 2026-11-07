//! Ventana de HidroMac Flow con egui: abre, edita el punto, guarda y exporta.
//!
//! Vive en su propio workspace, con su job `ui-egui` en el CI.
//! Ejecutar con `cargo run --manifest-path crates/hm-ui-egui/Cargo.toml`.

mod bar;
mod curves;
mod diagnostics;
mod inputs;
mod inspector;
mod model;
mod plan;
mod session;
mod summary;

use eframe::egui;
use hm_formula::Registry;

use model::{FieldErrors, Outcome, compute};
use session::Session;

use bar::top_bar;
use curves::curves;
use diagnostics::diagnostics_panel;
use inputs::inputs_panel;
use inspector::inspector_panel;
use plan::plan_view;
use summary::summary;

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
