//! Primer corte de la UI de HidroMac Flow.
//!
//! Vive en su propio workspace y queda fuera del CI principal mientras se
//! evalúa iced. Ejecutar con:
//! `cargo run --manifest-path crates/hm-ui/Cargo.toml`.

use hm_core::{Diagnostic, Severity};
use hm_design::{CurvePoint, DesignInput, DesignResult, VoluteSection, design, performance_curve};
use hm_formula::Registry;
use hm_units::parse_quantity;
use iced::widget::canvas;
use iced::widget::canvas::{Path, Stroke};
use iced::widget::{Space, button, column, container, row, scrollable, table, text, text_input};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse,
};

fn main() -> iced::Result {
    iced::application(App::default, update, view)
        .title("HidroMac Flow")
        .window_size(iced::Size::new(1280.0, 820.0))
        .theme(hmf_theme())
        .run()
}

fn hmf_theme() -> Theme {
    Theme::custom(
        "HMF",
        iced::theme::Palette {
            background: Color::from_rgb(0.043, 0.055, 0.067),
            text: Color::from_rgb(0.87, 0.90, 0.92),
            primary: Color::from_rgb(0.18, 0.55, 0.73),
            success: Color::from_rgb(0.24, 0.72, 0.50),
            warning: Color::from_rgb(0.90, 0.65, 0.25),
            danger: Color::from_rgb(0.85, 0.32, 0.32),
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Diseno,
    Curvas,
    Voluta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selected {
    Q,
    H,
    N,
    Rho,
    EtaM,
    Npsha,
    Nq,
    Dn,
    Psi,
    Phi,
    Beta2,
    Blades,
    D2,
    B2,
    D1,
    B1,
    U2,
    Eta,
    EtaH,
    EtaV,
    PHyd,
    PShaft,
    Torque,
    Npshr,
    H0,
    R3,
    B3,
    R4,
    A360,
}

#[derive(Debug)]
struct App {
    q: String,
    h: String,
    n: String,
    rho: String,
    eta_m: String,
    npsha: String,
    result: Option<DesignResult>,
    curve: Vec<CurvePoint>,
    diagnostics: Vec<Diagnostic>,
    error: Option<String>,
    selected: Selected,
    tab: Tab,
    registry: Registry,
}

impl Default for App {
    fn default() -> Self {
        Self {
            q: "120 m3/h".to_string(),
            h: "45 m".to_string(),
            n: "2900 rpm".to_string(),
            rho: "998.2 kg/m3".to_string(),
            eta_m: "0.96".to_string(),
            npsha: "15 m".to_string(),
            result: None,
            curve: Vec::new(),
            diagnostics: Vec::new(),
            error: None,
            selected: Selected::D2,
            tab: Tab::Diseno,
            registry: Registry::all(),
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Q(String),
    H(String),
    N(String),
    Rho(String),
    Eta(String),
    Npsha(String),
    Select(Selected),
    Tab(Tab),
    Design,
}

fn update(app: &mut App, message: Message) {
    match message {
        Message::Q(value) => app.q = value,
        Message::H(value) => app.h = value,
        Message::N(value) => app.n = value,
        Message::Rho(value) => app.rho = value,
        Message::Eta(value) => app.eta_m = value,
        Message::Npsha(value) => app.npsha = value,
        Message::Select(selected) => app.selected = selected,
        Message::Tab(tab) => app.tab = tab,
        Message::Design => app.run_design(),
    }
}

impl App {
    fn run_design(&mut self) {
        self.error = None;
        self.result = None;
        self.curve.clear();
        self.diagnostics.clear();

        match self.compute() {
            Ok((result, curve, diagnostics)) => {
                self.result = Some(result);
                self.curve = curve;
                self.diagnostics = diagnostics;
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn compute(&self) -> Result<(DesignResult, Vec<CurvePoint>, Vec<Diagnostic>), String> {
        let eta_m = self
            .eta_m
            .trim()
            .parse::<f64>()
            .map_err(|error| format!("ηm inválida: {error}"))?;

        let input = DesignInput {
            q: parse_quantity(&self.q).map_err(|error| error.to_string())?,
            h: parse_quantity(&self.h).map_err(|error| error.to_string())?,
            n: parse_quantity(&self.n).map_err(|error| error.to_string())?,
            rho: parse_quantity(&self.rho).map_err(|error| error.to_string())?,
            eta_m,
            npsha: Some(parse_quantity(&self.npsha).map_err(|error| error.to_string())?),
        };

        let result = design(&input).map_err(|error| error.to_string())?;
        let curve = performance_curve(&input, &result, 11);
        let diagnostics = result.diagnostics.clone().into_vec();

        Ok((result, curve, diagnostics))
    }
}

fn view(app: &App) -> Element<'_, Message> {
    let body = row![machine_map(app), workbench(app), inspector(app)]
        .spacing(10)
        .height(Length::Fill);

    container(column![top_bar(app), body, ledger(app)].spacing(10))
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(root_style)
        .into()
}

fn top_bar(app: &App) -> Element<'_, Message> {
    let (status, color) = status(app);
    let left = column![
        text("HIDROMAC FLOW").size(20),
        text("Diseño preliminar · end-suction OH1/OH2")
            .size(11)
            .color(muted()),
    ]
    .spacing(2);

    container(
        row![
            left,
            Space::new().width(Length::Fill),
            text(status).size(12).color(color),
        ]
        .align_y(Alignment::Center),
    )
    .padding(12)
    .width(Length::Fill)
    .style(top_bar_style)
    .into()
}

fn status(app: &App) -> (&'static str, Color) {
    if app.error.is_some() || app.diagnostics.iter().any(Diagnostic::is_error) {
        ("ERROR", danger())
    } else if app
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Warning)
    {
        ("REVISAR", warning())
    } else if app.result.is_some() {
        ("VÁLIDO", success())
    } else {
        ("LISTO", muted())
    }
}

fn panel<'a>(content: impl Into<Element<'a, Message>>, width: Length) -> Element<'a, Message> {
    container(content)
        .padding(14)
        .width(width)
        .height(Length::Fill)
        .style(panel_style)
        .into()
}

fn section<'a>(title: &'a str, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(title).size(11).color(muted()), content.into(),]
        .spacing(6)
        .into()
}

fn machine_map(app: &App) -> Element<'_, Message> {
    let active = if app.result.is_some() {
        success()
    } else {
        muted()
    };
    let mut parts = column![
        text("MÁQUINA").size(11).color(muted()),
        text("Etapa 1").size(17),
    ]
    .spacing(10);

    for part in ["Impulsor", "Voluta", "Sello", "Cojinetes"] {
        parts = parts.push(row![text("●").size(10).color(active), text(part).size(13),].spacing(6));
    }

    parts = parts.push(text("Mapa geométrico pendiente").size(11).color(muted()));

    panel(scrollable(parts), Length::Fixed(200.0))
}

fn workbench(app: &App) -> Element<'_, Message> {
    let tabs = row![
        tab_button("Diseño", Tab::Diseno, app.tab),
        tab_button("Curvas", Tab::Curvas, app.tab),
        tab_button("Voluta", Tab::Voluta, app.tab),
    ]
    .spacing(4);

    let header = row![
        text("BANCO DE TRABAJO").size(11).color(muted()),
        Space::new().width(Length::Fill),
        tabs,
    ]
    .align_y(Alignment::Center);

    let content = match app.tab {
        Tab::Diseno => diseno_content(app),
        Tab::Curvas => curvas_content(app),
        Tab::Voluta => voluta_content(app),
    };

    panel(
        column![header, scrollable(content).height(Length::Fill)].spacing(12),
        Length::Fill,
    )
}

fn tab_button(label: &str, tab: Tab, current: Tab) -> Element<'_, Message> {
    let style: fn(&Theme, button::Status) -> button::Style = if tab == current {
        button::primary
    } else {
        button::text
    };

    button(text(label).size(12))
        .on_press(Message::Tab(tab))
        .style(style)
        .into()
}

fn diseno_content(app: &App) -> Element<'_, Message> {
    let inputs = column![
        row![
            field("Q", &app.q, Message::Q),
            field("H", &app.h, Message::H),
            field("n", &app.n, Message::N),
        ]
        .spacing(10),
        row![
            field("ρ", &app.rho, Message::Rho),
            field("ηm", &app.eta_m, Message::Eta),
            field("NPSHa", &app.npsha, Message::Npsha),
        ]
        .spacing(10),
        button(text("Diseñar").size(13))
            .on_press(Message::Design)
            .style(button::primary),
    ]
    .spacing(10);

    let mut content = column![section("Punto de diseño", inputs)].spacing(14);

    if let Some(error) = &app.error {
        content = content.push(section(
            "Error",
            text(error.clone()).size(13).color(danger()),
        ));
    }

    if let Some(result) = &app.result {
        content = content.push(section("Resultado", results(result)));
        content = content.push(section("Parámetros", parameters(app)));
    }

    content.into()
}

fn curvas_content(app: &App) -> Element<'_, Message> {
    if app.result.is_none() || app.curve.is_empty() {
        return placeholder("Diseña para ver las curvas.");
    }

    let plot = canvas(CurvePlot {
        points: app.curve.clone(),
    })
    .width(Length::Fill)
    .height(Length::Fixed(420.0));

    column![
        section("H(Q) y NPSHr(Q)", plot),
        section("Puntos", curve_table(app)),
    ]
    .spacing(14)
    .into()
}

fn voluta_content(app: &App) -> Element<'_, Message> {
    let Some(result) = &app.result else {
        return placeholder("Diseña para ver la voluta.");
    };

    let summary = text(format!(
        "R3 = {:.1} mm   B3 = {:.1} mm   C3 = {:.2} m/s   A360 = {:.6} m²   D360 = {:.1} mm   R4 = {:.1} mm",
        result.volute.base_radius.value() * 1000.0,
        result.volute.width.value() * 1000.0,
        result.volute.inlet_velocity.value(),
        result.volute.throat_area.value(),
        result.volute.throat_diameter.value() * 1000.0,
        result.volute.outer_radius.value() * 1000.0,
    ))
    .size(13);

    column![
        section("Geometría de garganta", summary),
        section("Secciones", volute_table(result)),
    ]
    .spacing(14)
    .into()
}

fn placeholder(message: &str) -> Element<'_, Message> {
    container(text(message).size(13).color(muted()))
        .padding(20)
        .into()
}

fn curve_table(app: &App) -> Element<'_, Message> {
    table::table(
        [
            table::column(
                text("Q m³/h").size(11).color(muted()),
                |point: CurvePoint| text(format!("{:.1}", point.q.value())).size(12),
            ),
            table::column(text("H m").size(11).color(muted()), |point: CurvePoint| {
                text(format!("{:.2}", point.h.value())).size(12)
            }),
            table::column(text("η").size(11).color(muted()), |point: CurvePoint| {
                text(format!("{:.3}", point.eta)).size(12)
            }),
            table::column(text("P kW").size(11).color(muted()), |point: CurvePoint| {
                text(format!("{:.2}", point.p_shaft.value())).size(12)
            }),
            table::column(
                text("NPSHr m").size(11).color(muted()),
                |point: CurvePoint| text(format!("{:.2}", point.npshr.value())).size(12),
            ),
        ],
        app.curve.clone(),
    )
    .width(Length::Fill)
    .padding(6.0)
    .into()
}

fn volute_table(result: &DesignResult) -> Element<'_, Message> {
    table::table(
        [
            table::column(
                text("θ °").size(11).color(muted()),
                |section: VoluteSection| text(format!("{:.0}", section.angle_deg)).size(12),
            ),
            table::column(
                text("A m²").size(11).color(muted()),
                |section: VoluteSection| text(format!("{:.5}", section.area.value())).size(12),
            ),
            table::column(
                text("D mm").size(11).color(muted()),
                |section: VoluteSection| {
                    text(format!("{:.1}", section.diameter.value() * 1000.0)).size(12)
                },
            ),
            table::column(
                text("R ext mm").size(11).color(muted()),
                |section: VoluteSection| {
                    text(format!("{:.1}", section.outer_radius.value() * 1000.0)).size(12)
                },
            ),
        ],
        result.volute.sections.clone(),
    )
    .width(Length::Fill)
    .padding(6.0)
    .into()
}

fn inspector(app: &App) -> Element<'_, Message> {
    let mut content = column![text("INSPECTOR").size(11).color(muted())].spacing(10);

    if let Some(result) = &app.result {
        let (title, value, formula) = selected_info(app, result);
        content = content.push(text(title).size(12).color(muted()));
        content = content.push(text(value).size(24));
        content = content.push(
            text(if formula.is_some() {
                "Calculado · auto"
            } else {
                "Entrada / selección"
            })
            .size(11)
            .color(muted()),
        );

        if let Some(formula) = formula
            && let Some(spec) = app.registry.get(formula)
        {
            content = content.push(section("Fórmula", text(spec.title.clone()).size(13)));
            content = content.push(section(
                "Fuente",
                text(spec.source.clone()).size(12).color(muted()),
            ));
            content = content.push(section(
                "Rango",
                text(spec.range.clone()).size(12).color(muted()),
            ));
            if let Some(notes) = &spec.notes
                && let Some(assumptions) = &notes.assumptions
            {
                content = content.push(section(
                    "Supuestos",
                    text(assumptions.clone()).size(12).color(muted()),
                ));
            }
        }
    } else {
        content = content.push(text("Diseña para inspeccionar un parámetro."));
    }

    panel(scrollable(content), Length::Fixed(320.0))
}

fn ledger(app: &App) -> Element<'_, Message> {
    let header = row![
        text("LEDGER").size(11).color(muted()),
        Space::new().width(Length::Fill),
        text(format!("{} avisos", app.diagnostics.len()))
            .size(11)
            .color(muted()),
    ]
    .align_y(Alignment::Center);

    let mut content = column![header].spacing(6);

    if app.diagnostics.is_empty() {
        content = content.push(text("Sin cambios ni avisos.").size(12).color(muted()));
    } else {
        for diagnostic in &app.diagnostics {
            let color = diagnostic_color(diagnostic.severity);
            content = content.push(
                text(format!("{}  {}", diagnostic.code, diagnostic.message))
                    .size(12)
                    .color(color),
            );
            if let Some(hint) = &diagnostic.hint {
                content = content.push(text(format!("  {hint}")).size(11).color(muted()));
            }
        }
    }

    container(scrollable(content))
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fixed(160.0))
        .style(panel_style)
        .into()
}

fn diagnostic_color(severity: Severity) -> Color {
    match severity {
        Severity::Error | Severity::Blocking => danger(),
        Severity::Warning => warning(),
        Severity::Info => Color::from_rgb(0.55, 0.72, 0.85),
    }
}

fn muted() -> Color {
    Color::from_rgb(0.56, 0.63, 0.69)
}

fn success() -> Color {
    Color::from_rgb(0.24, 0.72, 0.50)
}

fn warning() -> Color {
    Color::from_rgb(0.90, 0.65, 0.25)
}

fn danger() -> Color {
    Color::from_rgb(0.85, 0.32, 0.32)
}

fn root_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme.palette().background)),
        ..Default::default()
    }
}

fn top_bar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.06, 0.08, 0.095))),
        border: Border {
            color: Color::from_rgb(0.13, 0.17, 0.20),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

fn panel_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgb(0.072, 0.088, 0.103))),
        border: Border {
            color: Color::from_rgb(0.13, 0.17, 0.20),
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

fn metric_button(label: &str, selected: Selected, current: Selected) -> Element<'_, Message> {
    let style: fn(&Theme, button::Status) -> button::Style = if selected == current {
        button::primary
    } else {
        button::text
    };

    button(text(label).size(11))
        .on_press(Message::Select(selected))
        .style(style)
        .into()
}

fn parameters(app: &App) -> Element<'_, Message> {
    let rows: &[&[Selected]] = &[
        &[Selected::Nq, Selected::Dn, Selected::D2, Selected::B2],
        &[Selected::D1, Selected::B1, Selected::U2, Selected::Eta],
        &[
            Selected::EtaH,
            Selected::EtaV,
            Selected::PHyd,
            Selected::PShaft,
        ],
        &[
            Selected::Torque,
            Selected::Npshr,
            Selected::H0,
            Selected::R3,
        ],
        &[Selected::B3, Selected::R4, Selected::A360, Selected::Psi],
        &[
            Selected::Phi,
            Selected::Beta2,
            Selected::Blades,
            Selected::Q,
        ],
        &[
            Selected::H,
            Selected::N,
            Selected::Rho,
            Selected::EtaM,
            Selected::Npsha,
        ],
    ];

    let mut content = column![text("Parámetros").size(14)].spacing(6);

    for row_items in rows {
        let mut line = row![].spacing(6);
        for &selected in *row_items {
            line = line.push(metric_button(label_for(selected), selected, app.selected));
        }
        content = content.push(line);
    }

    content.into()
}

fn label_for(selected: Selected) -> &'static str {
    match selected {
        Selected::Q => "Q",
        Selected::H => "H",
        Selected::N => "n",
        Selected::Rho => "rho",
        Selected::EtaM => "eta_m",
        Selected::Npsha => "NPSHa",
        Selected::Nq => "nq",
        Selected::Dn => "Dn",
        Selected::Psi => "psi",
        Selected::Phi => "phi",
        Selected::Beta2 => "beta2",
        Selected::Blades => "Z",
        Selected::D2 => "D2",
        Selected::B2 => "B2",
        Selected::D1 => "D1",
        Selected::B1 => "B1",
        Selected::U2 => "u2",
        Selected::Eta => "eta",
        Selected::EtaH => "eta_h",
        Selected::EtaV => "eta_v",
        Selected::PHyd => "P_hyd",
        Selected::PShaft => "P_eje",
        Selected::Torque => "T",
        Selected::Npshr => "NPSHr",
        Selected::H0 => "H0",
        Selected::R3 => "R3",
        Selected::B3 => "B3",
        Selected::R4 => "R4",
        Selected::A360 => "A360",
    }
}

fn selected_info(app: &App, result: &DesignResult) -> (&'static str, String, Option<&'static str>) {
    match app.selected {
        Selected::Q => ("Caudal de diseño", app.q.clone(), None),
        Selected::H => ("Altura de diseño", app.h.clone(), None),
        Selected::N => ("Velocidad de giro", app.n.clone(), None),
        Selected::Rho => ("Densidad", app.rho.clone(), None),
        Selected::EtaM => ("Eficiencia mecánica", app.eta_m.clone(), None),
        Selected::Npsha => ("NPSH disponible", app.npsha.clone(), None),
        Selected::Nq => (
            "Velocidad específica nq",
            format!("{:.3}", result.nq),
            Some("similarity.nq"),
        ),
        Selected::Dn => (
            "Diámetro específico",
            format!("{:.2} mm", result.specific_diameter_mm),
            Some("design.specific_diameter"),
        ),
        Selected::Psi => (
            "Coeficiente de altura",
            format!("{:.3}", result.selection.psi),
            None,
        ),
        Selected::Phi => (
            "Coeficiente de caudal",
            format!("{:.3}", result.selection.phi),
            None,
        ),
        Selected::Beta2 => (
            "Ángulo de salida del álabe",
            format!("{:.1}°", result.selection.beta2_deg),
            None,
        ),
        Selected::Blades => (
            "Número de álabes",
            format!("{}", result.selection.blade_count),
            None,
        ),
        Selected::D2 => (
            "Diámetro exterior",
            format!("{:.2} mm", result.d2.value() * 1000.0),
            Some("design.d2"),
        ),
        Selected::B2 => (
            "Ancho de salida",
            format!("{:.2} mm", result.b2.value() * 1000.0),
            Some("design.b2"),
        ),
        Selected::D1 => (
            "Diámetro de entrada",
            format!("{:.2} mm", result.d1.value() * 1000.0),
            Some("design.d1"),
        ),
        Selected::B1 => (
            "Ancho de entrada",
            format!("{:.2} mm", result.b1.value() * 1000.0),
            Some("design.b1"),
        ),
        Selected::U2 => (
            "Velocidad periférica",
            format!("{:.2} m/s", result.u2.value()),
            Some("design.u2"),
        ),
        Selected::Eta => (
            "Eficiencia total",
            format!("{:.3}", result.eta),
            Some("design.overall_efficiency"),
        ),
        Selected::EtaH => (
            "Eficiencia hidráulica",
            format!("{:.3}", result.eta_h),
            Some("design.hydraulic_efficiency"),
        ),
        Selected::EtaV => (
            "Eficiencia volumétrica",
            format!("{:.3}", result.eta_v),
            Some("design.volumetric_efficiency"),
        ),
        Selected::PHyd => (
            "Potencia hidráulica",
            format!("{:.0} W", result.p_hyd.value()),
            Some("perf.hydraulic_power"),
        ),
        Selected::PShaft => (
            "Potencia en el eje",
            format!("{:.0} W", result.p_shaft.value()),
            Some("design.shaft_power"),
        ),
        Selected::Torque => (
            "Par en el eje",
            format!("{:.1} N·m", result.torque.value()),
            Some("mech.shaft_torque"),
        ),
        Selected::Npshr => (
            "NPSHr de diseño",
            format!("{:.2} m", result.npshr_design.value()),
            Some("design.npshr_design"),
        ),
        Selected::H0 => (
            "Altura a caudal cero",
            format!("{:.2} m", result.shutoff_head.value()),
            Some("design.shutoff_head"),
        ),
        Selected::R3 => (
            "Radio de la voluta",
            format!("{:.1} mm", result.volute.base_radius.value() * 1000.0),
            Some("design.volute_base_radius"),
        ),
        Selected::B3 => (
            "Ancho de la voluta",
            format!("{:.1} mm", result.volute.width.value() * 1000.0),
            Some("design.volute_width"),
        ),
        Selected::R4 => (
            "Radio exterior de la voluta",
            format!("{:.1} mm", result.volute.outer_radius.value() * 1000.0),
            Some("design.volute_outer_radius"),
        ),
        Selected::A360 => (
            "Área de garganta",
            format!("{:.6} m²", result.volute.throat_area.value()),
            Some("design.volute_throat_area"),
        ),
    }
}

fn field<'a>(
    label: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(label).size(10).color(muted()),
        text_input("", value).on_input(on_input).size(13).padding(8),
    ]
    .spacing(4)
    .width(Length::FillPortion(1))
    .into()
}

fn results(result: &DesignResult) -> Element<'_, Message> {
    let geometry = column![
        text("Geometría").size(18),
        text(format!(
            "nq = {:.2}   ψ = {:.3}   φ = {:.3}   β2 = {:.1}°   Z = {}",
            result.nq,
            result.selection.psi,
            result.selection.phi,
            result.selection.beta2_deg,
            result.selection.blade_count
        )),
        text(format!(
            "D2 = {:.1} mm   B2 = {:.1} mm   u2 = {:.2} m/s",
            result.d2.value() * 1000.0,
            result.b2.value() * 1000.0,
            result.u2.value()
        )),
        text(format!(
            "D0 = {:.1} mm   D1 = {:.1} mm   B1 = {:.1} mm",
            result.d0.value() * 1000.0,
            result.d1.value() * 1000.0,
            result.b1.value() * 1000.0
        )),
        text(format!(
            "η = {:.3}   P_eje = {:.0} W   T = {:.1} N·m",
            result.eta,
            result.p_shaft.value(),
            result.torque.value()
        )),
        text(format!(
            "Voluta: R3 = {:.1} mm   B3 = {:.1} mm   R4 = {:.1} mm",
            result.volute.base_radius.value() * 1000.0,
            result.volute.width.value() * 1000.0,
            result.volute.outer_radius.value() * 1000.0
        )),
        text(format!(
            "Pérdidas: hidráulica {:.0} W   volumétrica {:.0} W   mecánica {:.0} W",
            result.p_hyd_loss.value(),
            result.p_vol_loss.value(),
            result.p_mech_loss.value()
        )),
        text(format!(
            "NPSHr_d = {:.2} m   H0 = {:.2} m",
            result.npshr_design.value(),
            result.shutoff_head.value()
        )),
    ]
    .spacing(6);

    container(geometry)
        .padding(16)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

#[derive(Debug)]
struct CurvePlot {
    points: Vec<CurvePoint>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_design_computes() {
        let app = App::default();
        let (result, curve, diagnostics) = app.compute().unwrap();

        assert_eq!(curve.len(), 11);
        assert!(!result.diagnostics.has_errors());
        assert!(diagnostics.iter().all(|diagnostic| !diagnostic.is_error()));
    }

    #[test]
    fn invalid_efficiency_is_reported() {
        let mut app = App::default();
        app.eta_m = "no".to_string();
        assert!(app.compute().is_err());
    }

    #[test]
    fn invalid_npsha_is_reported() {
        let mut app = App::default();
        app.npsha = "mucho".to_string();
        assert!(app.compute().is_err());
    }
}

impl<Message> canvas::Program<Message> for CurvePlot {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        if self.points.len() < 2 {
            return vec![frame.into_geometry()];
        }

        let q_min = self.points.first().map(|p| p.q.value()).unwrap_or(0.0);
        let q_max = self.points.last().map(|p| p.q.value()).unwrap_or(1.0);
        let h_max = self
            .points
            .iter()
            .map(|p| p.h.value())
            .fold(0.0_f64, f64::max)
            .max(1.0);
        let npsh_max = self
            .points
            .iter()
            .map(|p| p.npshr.value())
            .fold(0.0_f64, f64::max)
            .max(1.0);

        let left = 32.0;
        let right = bounds.width - 12.0;
        let top = 12.0;
        let bottom = bounds.height - 24.0;
        let x = |q: f64| left + ((q - q_min) / (q_max - q_min).max(1e-9)) as f32 * (right - left);
        let y = |value: f64, max: f64| bottom - (value / max.max(1e-9)) as f32 * (bottom - top);

        for index in 0..=4 {
            let t = index as f32 / 4.0;
            let grid_x = left + t * (right - left);
            let grid_y = top + t * (bottom - top);

            let vertical = Path::line(Point::new(grid_x, top), Point::new(grid_x, bottom));
            frame.stroke(
                &vertical,
                Stroke::default()
                    .with_color(Color::from_rgba(0.55, 0.6, 0.65, 0.18))
                    .with_width(1.0),
            );

            let horizontal = Path::line(Point::new(left, grid_y), Point::new(right, grid_y));
            frame.stroke(
                &horizontal,
                Stroke::default()
                    .with_color(Color::from_rgba(0.55, 0.6, 0.65, 0.18))
                    .with_width(1.0),
            );
        }

        let axis = Path::new(|builder| {
            builder.move_to(Point::new(left, top));
            builder.line_to(Point::new(left, bottom));
            builder.line_to(Point::new(right, bottom));
        });
        frame.stroke(
            &axis,
            Stroke::default()
                .with_color(Color::from_rgb(0.55, 0.55, 0.55))
                .with_width(1.0),
        );

        let height_path = Path::new(|builder| {
            if let Some(first) = self.points.first() {
                builder.move_to(Point::new(x(first.q.value()), y(first.h.value(), h_max)));
                for point in self.points.iter().skip(1) {
                    builder.line_to(Point::new(x(point.q.value()), y(point.h.value(), h_max)));
                }
            }
        });
        frame.stroke(
            &height_path,
            Stroke::default()
                .with_color(Color::from_rgb(0.25, 0.65, 1.0))
                .with_width(2.0),
        );

        let npsh_path = Path::new(|builder| {
            if let Some(first) = self.points.first() {
                builder.move_to(Point::new(
                    x(first.q.value()),
                    y(first.npshr.value(), npsh_max),
                ));
                for point in self.points.iter().skip(1) {
                    builder.line_to(Point::new(
                        x(point.q.value()),
                        y(point.npshr.value(), npsh_max),
                    ));
                }
            }
        });
        frame.stroke(
            &npsh_path,
            Stroke::default()
                .with_color(Color::from_rgb(1.0, 0.65, 0.2))
                .with_width(2.0),
        );

        draw_text(
            &mut frame,
            "H(Q)",
            left + 8.0,
            top + 2.0,
            Color::from_rgb(0.25, 0.65, 1.0),
        );
        draw_text(
            &mut frame,
            "NPSHr(Q)",
            left + 62.0,
            top + 2.0,
            Color::from_rgb(1.0, 0.65, 0.2),
        );
        draw_text(
            &mut frame,
            &format!("{q_min:.0}"),
            left,
            bottom + 6.0,
            Color::from_rgb(0.65, 0.65, 0.65),
        );
        draw_text(
            &mut frame,
            &format!("{q_max:.0} m3/h"),
            right - 58.0,
            bottom + 6.0,
            Color::from_rgb(0.65, 0.65, 0.65),
        );
        draw_text(
            &mut frame,
            &format!("{h_max:.0} m"),
            left + 4.0,
            top + 18.0,
            Color::from_rgb(0.65, 0.65, 0.65),
        );

        vec![frame.into_geometry()]
    }
}

fn draw_text(frame: &mut canvas::Frame, content: &str, x: f32, y: f32, color: Color) {
    frame.fill_text(canvas::Text {
        content: content.to_string(),
        position: Point::new(x, y),
        color,
        size: 12.0.into(),
        ..Default::default()
    });
}
