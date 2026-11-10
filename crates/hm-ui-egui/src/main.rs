//! Ventana de HidroMac Flow con egui: abre, edita el punto, guarda y exporta.
//!
//! Vive en su propio workspace, con su job `ui-egui` en el CI.
//! Ejecutar con `cargo run --manifest-path crates/hm-ui-egui/Cargo.toml`.

mod bar;
mod blade_beta;
mod blade_development;
mod blade_inlet;
mod curves;
mod diagnostics;
mod inputs;
mod inspector;
mod meridian;
mod meridian_arc;
mod meridian_constraints;
mod meridian_fillet;
mod meridian_segments;
mod model;
mod plan;
mod session;
mod summary;
mod theme;
mod view3d;

use eframe::egui;
use hm_formula::Registry;

use model::{FieldErrors, Outcome, compute};
use session::Session;

use bar::top_bar;
use curves::curves;
use diagnostics::diagnostics_panel;
use inputs::inputs_panel;
use inspector::inspector_panel;
use meridian::meridian_view;
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
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            if let Some(preference) =
                theme::theme_from_env(std::env::var("HMF_THEME").ok().as_deref())
            {
                cc.egui_ctx.set_theme(preference);
            }
            Ok(Box::new(App::new()))
        }),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Summary,
    Curves,
    Geometry,
    Meridian,
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
    /// Deshacer y rehacer de la pestaña Meridiano.
    meridian_history: meridian::History,
    beta_history: blade_beta::BetaHistory,
}

/// `HMF_SCREENSHOT=ruta.ppm` (y opcionalmente `HMF_TAB=curves|geometry|meridian`)
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
                Ok("meridian") => Tab::Meridian,
                _ => Tab::Summary,
            },
            registry: Registry::all(),
            capture: std::env::var_os("HMF_SCREENSHOT").map(|path| Capture {
                path: path.into(),
                frames: 0,
                done: Default::default(),
            }),
            reselect_requested: false,
            meridian_history: meridian::History::default(),
            beta_history: blade_beta::BetaHistory::default(),
        };
        // Solo para las capturas: `HMF_MERIDIAN="hub_diameter=0 mm;edge_hub=0.4"`.
        if let Ok(text) = std::env::var("HMF_MERIDIAN") {
            let boxes = &mut app.session.inputs.meridian;
            for pair in text.split(';') {
                match pair.split_once('=') {
                    Some(("hub_diameter", value)) => boxes.hub_diameter = value.trim().into(),
                    Some(("edge_shroud", value)) => boxes.edge_shroud = value.trim().into(),
                    Some(("edge_hub", value)) => boxes.edge_hub = value.trim().into(),
                    Some(("edge_p2_along", value)) => boxes.edge.p2_along = value.trim().into(),
                    Some(("edge_p2_offset", value)) => boxes.edge.p2_offset = value.trim().into(),
                    Some(("edge_start_angle", value)) => {
                        boxes.edge.start_angle_deg = value.trim().into();
                    }
                    Some(("edge_start_length", value)) => {
                        boxes.edge.start_length = value.trim().into();
                    }
                    Some(("edge_end_angle", value)) => {
                        boxes.edge.end_angle_deg = value.trim().into()
                    }
                    Some(("edge_end_length", value)) => boxes.edge.end_length = value.trim().into(),
                    _ => eprintln!("HMF_MERIDIAN: no entiendo `{pair}`"),
                }
            }
        }
        blade_beta::seed_from_env(&mut app.session.inputs);
        app.recompute();
        app
    }

    /// El diseño tarda milisegundos: se recalcula con cada cambio.
    fn recompute(&mut self) {
        self.recompute_passes(3);
    }

    /// `passes`: cuántas veces más se puede recalcular porque las cotas que
    /// mandan movieron algo. Lo normal es una; el tope evita dar vueltas.
    fn recompute_passes(&mut self, passes: u8) {
        match compute(&self.session.inputs, &self.session.basis) {
            Ok(outcome) => {
                // Las cotas que mandan siguen al perfil nuevo aunque la pestaña
                // Meridiano esté cerrada. Los nodos libres no entran en el
                // cálculo, pero `outcome.inputs` tiene que coincidir.
                let before = self.session.inputs.meridian.clone();
                if passes > 0
                    && meridian::enforce_constraints(&outcome, &mut self.session.inputs.meridian)
                {
                    // Consecuencia del cambio que ya anotó el deshacer.
                    self.meridian_history
                        .amend(&before, &self.session.inputs.meridian);
                    return self.recompute_passes(passes - 1);
                }
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
        let mut meridian_edited = false;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Summary, "Resumen");
                ui.selectable_value(&mut self.tab, Tab::Curves, "Curvas");
                ui.selectable_value(&mut self.tab, Tab::Geometry, "Geometría");
                ui.selectable_value(&mut self.tab, Tab::Meridian, "Meridiano");
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
                Tab::Geometry => {
                    if blade_beta::blade_beta_view(
                        ui,
                        outcome,
                        &mut self.session.inputs,
                        &mut self.beta_history,
                    ) {
                        meridian_edited = true;
                    }
                    plan_view(ui, outcome);
                }
                Tab::Meridian => {
                    if meridian_view(
                        ui,
                        outcome,
                        &mut self.session.inputs.meridian,
                        &mut self.meridian_history,
                    ) {
                        meridian_edited = true;
                    }
                }
            }
        });
        // Fuera del panel, que tiene prestado el resultado: sin recalcular,
        // la barra diría «entradas inválidas» tras cada edición del meridiano.
        if meridian_edited {
            self.session.edited();
            self.recompute();
        }
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
