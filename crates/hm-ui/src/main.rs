//! Primer corte de la UI de HidroMac Flow.
//!
//! Vive en su propio workspace y queda fuera del CI principal mientras se
//! evalúa iced. Ejecutar con:
//! `cargo run --manifest-path crates/hm-ui/Cargo.toml`.

use hm_core::Diagnostic;
use hm_design::{CurvePoint, DesignInput, DesignResult, design, performance_curve};
use hm_units::parse_quantity;
use iced::widget::canvas;
use iced::widget::canvas::{Path, Stroke};
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

fn main() -> iced::Result {
    iced::application(App::default, update, view)
        .title("HidroMac Flow")
        .window_size(iced::Size::new(1200.0, 800.0))
        .run()
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
    let title = column![
        text("HidroMac Flow").size(30),
        text("Diseño preliminar end-suction OH1/OH2").size(16),
    ]
    .spacing(2);

    let inputs = column![
        row![
            field("Q", &app.q, Message::Q),
            field("H", &app.h, Message::H),
            field("n", &app.n, Message::N),
        ]
        .spacing(12),
        row![
            field("ρ", &app.rho, Message::Rho),
            field("ηm", &app.eta_m, Message::Eta),
            field("NPSHa", &app.npsha, Message::Npsha),
        ]
        .spacing(12),
        button("Diseñar").on_press(Message::Design),
    ]
    .spacing(12);

    let mut content = column![title, inputs].spacing(12);

    if let Some(error) = &app.error {
        content =
            content.push(text(format!("Error: {error}")).color(Color::from_rgb(1.0, 0.4, 0.4)));
    }

    if let Some(result) = &app.result {
        content = content.push(results(result));
        if !app.curve.is_empty() {
            content = content.push(
                canvas(CurvePlot {
                    points: app.curve.clone(),
                })
                .width(Length::Fill)
                .height(Length::Fixed(260.0)),
            );
        }
        content = content.push(diagnostics(&app.diagnostics));
    }

    container(scrollable(content).width(Length::Fill))
        .padding(24)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn field<'a>(
    label: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(label).size(12),
        text_input("", value).on_input(on_input),
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

fn diagnostics(diagnostics: &[Diagnostic]) -> Element<'_, Message> {
    let mut list = column![text("Diagnósticos").size(18)].spacing(4);

    if diagnostics.is_empty() {
        list = list.push(text("Sin avisos."));
    } else {
        for diagnostic in diagnostics {
            list = list.push(text(format!(
                "[{:?}] {}: {}",
                diagnostic.severity, diagnostic.code, diagnostic.message
            )));
        }
    }

    container(list)
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

        vec![frame.into_geometry()]
    }
}
