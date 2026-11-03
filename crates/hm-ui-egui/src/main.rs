//! Prueba de la interfaz de HidroMac Flow con egui.
//!
//! Vive en su propio workspace y todavía no entra en el CI principal.
//! Ejecutar con `cargo run --manifest-path crates/hm-ui-egui/Cargo.toml`.

mod model;

use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Corner, HLine, Legend, Line, Plot, PlotPoints, Points, Polygon, VLine};
use hm_core::Severity;
use hm_design::{PARAM_SPECS, PREFERRED_OPERATING_REGION, designed_si};
use hm_formula::Registry;

use model::{Field, FieldErrors, Inputs, Outcome, compute};

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
    inputs: Inputs,
    outcome: Option<Outcome>,
    errors: FieldErrors,
    selected: &'static str,
    tab: Tab,
    registry: Registry,
    /// Captura de pantalla para revisar la interfaz sin abrirla a mano.
    capture: Option<Capture>,
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
            inputs: Inputs::default(),
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
        };
        app.recompute();
        app
    }

    /// El diseño tarda milisegundos: se recalcula con cada cambio.
    fn recompute(&mut self) {
        match compute(&self.inputs) {
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
            .is_some_and(|outcome| outcome.inputs != self.inputs)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(capture) = &mut self.capture {
            capture_step(ui.ctx(), capture);
        }
        top_bar(ui, self);
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

fn top_bar(ui: &mut egui::Ui, app: &App) {
    egui::Panel::top("top").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("HidroMac Flow");
            ui.label(
                RichText::new(format!(
                    "Diseño preliminar end-suction · método {}",
                    hm_design::METHOD
                ))
                .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, color) = status(ui, app);
                ui.label(RichText::new(text).strong().color(color));
            });
        });
    });
}

fn status(ui: &egui::Ui, app: &App) -> (&'static str, Color32) {
    let visuals = ui.visuals();
    let Some(outcome) = &app.outcome else {
        return ("SIN DISEÑO", visuals.error_fg_color);
    };
    if !app.errors.is_empty() || app.stale() {
        (
            "ENTRADAS INVÁLIDAS · resultados anteriores",
            visuals.error_fg_color,
        )
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
            ui.heading("Punto de diseño");
            ui.add_space(4.0);
            let mut changed = false;
            let errors = app.errors.clone();
            for (field, label, hint) in [
                (Field::Q, "Caudal Q", "120 m3/h"),
                (Field::H, "Altura H", "45 m"),
                (Field::N, "Velocidad n", "2900 rpm"),
                (Field::Temperature, "Temperatura del agua, °C", "20"),
                (Field::EtaM, "ηm cojinetes y cierre (vacío: Gülich)", "auto"),
                (
                    Field::Npsha,
                    "NPSH disponible (opcional)",
                    "vacío si no se conoce",
                ),
            ] {
                let value = match field {
                    Field::Q => &mut app.inputs.q,
                    Field::H => &mut app.inputs.h,
                    Field::N => &mut app.inputs.n,
                    Field::Temperature => &mut app.inputs.temperature_c,
                    Field::EtaM => &mut app.inputs.eta_m,
                    Field::Npsha => &mut app.inputs.npsha,
                };
                ui.label(label);
                let error = errors.iter().find(|(which, _)| *which == field);
                let mut edit = egui::TextEdit::singleline(value).hint_text(hint);
                if error.is_some() {
                    edit = edit.text_color(ui.visuals().error_fg_color);
                }
                changed |= ui.add(edit).changed();
                if let Some((_, message)) = error {
                    ui.label(
                        RichText::new(message)
                            .small()
                            .color(ui.visuals().error_fg_color),
                    );
                }
                ui.add_space(4.0);
            }
            if changed {
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
                if let Some(viscous) = &outcome.viscous {
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
        });
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
        if let Some(value) = designed_si(result, spec.name) {
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
    if let Some(mech) = &outcome.mechanics {
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
                "Empuje radial, válvula cerrada",
                format_si(mech.radial_thrust_shutoff, "N"),
                Some("mech.radial_thrust"),
            ),
            (
                "mech.radial1",
                "Empuje radial, diseño",
                format_si(mech.radial_thrust_design, "N"),
                Some("mech.radial_thrust"),
            ),
            (
                "mech.axial",
                "Empuje axial",
                format!("{:.0}..{:.0} N", mech.axial_thrust.0, mech.axial_thrust.1),
                Some("mech.axial_thrust_estimate"),
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

    vec![
        ("Diseño", design),
        ("Hidráulica fina", hydraulics),
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
                ui.label("Selección por tabla de nq; sin fórmula.");
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
            let Some(outcome) = &app.outcome else {
                return;
            };
            let count = |severity: Severity| {
                outcome
                    .diagnostics
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
                if let Some(error) = &outcome.geometry_error {
                    ui.label(
                        RichText::new(format!("geometría no disponible: {error}"))
                            .color(ui.visuals().warn_fg_color),
                    );
                }
                for diagnostic in &outcome.diagnostics {
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
                .and_then(|mech| mech.motor_rating)
            {
                plot_ui.hline(HLine::new("Motor IEC", rating / 1000.0).color(Color32::RED));
            }
        });
}

// ---------------------------------------------------------------- geometría

fn circle(radius: f64) -> Vec<[f64; 2]> {
    (0..=120)
        .map(|i| {
            let angle = std::f64::consts::TAU * f64::from(i) / 120.0;
            [radius * angle.cos(), radius * angle.sin()]
        })
        .collect()
}

fn plan_view(ui: &mut egui::Ui, outcome: &Outcome) {
    let Some(shape) = &outcome.geometry else {
        ui.label("Sin geometría para este punto.");
        return;
    };
    let result = &outcome.result;
    let mm = 1000.0;
    let blades = result.selection.blade_count;
    let blue = Color32::from_rgb(70, 150, 240);
    let grey = Color32::GRAY;
    let orange = Color32::from_rgb(240, 160, 50);

    // Voluta: del radio base en la lengua (0°) a los radios exteriores de
    // las secciones cada 45°, interpolados cada 3° para que no salga un
    // polígono.
    let mut stations = vec![(0.0, result.volute.base_radius.si)];
    stations.extend(
        result
            .volute
            .sections
            .iter()
            .map(|section| (section.angle_deg, section.outer_radius.si)),
    );
    let mut volute = Vec::new();
    for pair in stations.windows(2) {
        let ((a0, r0), (a1, r1)) = (pair[0], pair[1]);
        let steps = ((a1 - a0) / 3.0).ceil().max(1.0) as usize;
        for step in 0..steps {
            let t = step as f64 / steps as f64;
            let angle = (a0 + (a1 - a0) * t).to_radians();
            let radius = (r0 + (r1 - r0) * t) * mm;
            volute.push([radius * angle.cos(), radius * angle.sin()]);
        }
    }
    if let Some(&(angle, radius)) = stations.last() {
        let angle = angle.to_radians();
        volute.push([radius * mm * angle.cos(), radius * mm * angle.sin()]);
    }

    ui.label(
        RichText::new(format!(
            "Vista en planta, mm. Z = {blades}, β1B = {:.1}°, β2 = {:.1}°, envolvimiento = {:.0}°. \
             Álabe 2D (línea media); voluta por sus radios exteriores cada 45°, interpolados.",
            shape.impeller.beta1b.to_degrees(),
            result.selection.beta2_deg,
            shape.impeller.wrap_angle.to_degrees()
        ))
        .small()
        .weak(),
    );
    Plot::new("plan")
        .data_aspect(1.0)
        .legend(Legend::default())
        .show(ui, |plot_ui| {
            plot_ui.line(line("D1", circle(result.d1.si / 2.0 * mm), grey).width(1.0));
            plot_ui.line(line("D2", circle(result.d2.si / 2.0 * mm), grey).width(1.0));
            plot_ui.line(
                line(
                    "Lengua",
                    circle(shape.outlet.cutwater_radius * mm),
                    Color32::from_gray(90),
                )
                .width(1.0)
                .style(egui_plot::LineStyle::dashed_loose()),
            );
            for blade in 0..blades {
                let offset = std::f64::consts::TAU * f64::from(blade) / f64::from(blades);
                let points: Vec<[f64; 2]> = shape
                    .impeller
                    .blade
                    .iter()
                    .map(|point| {
                        let angle = offset - point.theta;
                        [point.r * mm * angle.cos(), point.r * mm * angle.sin()]
                    })
                    .collect();
                plot_ui.line(line("Álabes", points, blue));
            }
            plot_ui.line(line("Voluta", volute, orange));
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
