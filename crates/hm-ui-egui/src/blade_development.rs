//! Desarrollo conforme del álabe 3D: cada línea de corriente en el plano
//! (θ − Θ, m′), donde la pendiente es el β verdadero.

use eframe::egui;
use egui_plot::{Plot, Points};
use hm_design::blade_camber::BladeCamber;

use crate::curves::line;

/// Qué gráfica enseña el paso con forma 3D. Se guarda en los datos temporales
/// de egui.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum BladeChart {
    #[default]
    Beta,
    Development,
}

impl BladeChart {
    fn id() -> egui::Id {
        egui::Id::unique("blade_chart")
    }

    pub(super) fn stored(ctx: &egui::Context) -> Self {
        ctx.data(|data| data.get_temp::<Self>(Self::id()))
            .unwrap_or_default()
    }

    pub(super) fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(Self::id(), self));
    }
}

/// Una curva por línea, [(θ − Θ)°, (m′ − m′ del último nodo)·180/π]: todas
/// acaban en [0, 0].
pub(super) fn development_curves(camber: &BladeCamber) -> Vec<Vec<[f64; 2]>> {
    camber
        .lines
        .iter()
        .map(|line| {
            let last = line.points.last();
            let (theta_end, m_end) = last.map_or((0.0, 0.0), |p| (p.theta, p.m_conformal));
            line.points
                .iter()
                .map(|p| {
                    [
                        (p.theta - theta_end).to_degrees(),
                        (p.m_conformal - m_end).to_degrees(),
                    ]
                })
                .collect()
        })
        .collect()
}

/// El punto de las curvas más cercano al puntero: (línea, nodo).
fn nearest(curves: &[Vec<[f64; 2]>], at: [f64; 2]) -> Option<(usize, usize)> {
    curves
        .iter()
        .enumerate()
        .flat_map(|(k, curve)| curve.iter().enumerate().map(move |(j, p)| (k, j, p)))
        .min_by(|a, b| {
            let d = |p: &[f64; 2]| (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2);
            d(a.2).total_cmp(&d(b.2))
        })
        .map(|(k, j, _)| (k, j))
}

/// Solo lectura. Al pasar el ratón: línea, x y β del punto más cercano.
pub(super) fn development_view(ui: &mut egui::Ui, camber: &BladeCamber) {
    let curves = development_curves(camber);
    let palette = crate::theme::palette(ui.ctx());
    let last = curves.len().saturating_sub(1);
    let mut hover = None;
    Plot::new("blade_development")
        .height(190.0)
        .data_aspect(1.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .x_axis_label("θ − θ salida [°]")
        .y_axis_label("m′ [°]")
        .show(ui, |plot_ui| {
            if let Some(pointer) = plot_ui.pointer_coordinate() {
                hover = nearest(&curves, [pointer.x, pointer.y]);
            }
            for (index, curve) in curves.iter().enumerate() {
                let (name, color, width) = match index {
                    0 => ("Cubo", palette.hub, 2.5),
                    i if i == last => ("Tapa", palette.shroud, 2.5),
                    _ => ("Intermedias", palette.construction, 1.0),
                };
                plot_ui.line(line(name, curve.clone(), color).width(width));
            }
            let edges: Vec<[f64; 2]> = curves.iter().filter_map(|c| c.first().copied()).collect();
            plot_ui.points(
                Points::new("Borde de ataque", edges)
                    .color(palette.blade)
                    .radius(4.0),
            );
        });
    if let Some((k, j)) = hover
        && let Some(point) = camber.lines.get(k).and_then(|l| l.points.get(j))
    {
        let wall = match k {
            0 => " (cubo)",
            i if i == last => " (tapa)",
            _ => "",
        };
        ui.label(
            egui::RichText::new(format!(
                "Línea {}{wall} · x = {:.3} · β = {:.1}°",
                k + 1,
                point.x,
                point.beta.to_degrees()
            ))
            .small(),
        );
    }
    ui.label(
        egui::RichText::new(
            "Desarrollo conforme: la pendiente de cada línea es su β verdadero. Todas \
             acaban en la salida (0, 0).",
        )
        .small()
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Basis;
    use crate::model::Inputs;
    use hm_design::blade_setup::BladeShape;

    fn b1_camber() -> BladeCamber {
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free3D;
        inputs.blade.streamlines = 5;
        let outcome = crate::model::compute(&inputs, &Basis::default()).unwrap();
        crate::blade_inlet::camber_of(&outcome)
            .unwrap()
            .expect("forma 3D")
    }

    #[test]
    fn every_line_ends_at_the_origin() {
        let camber = b1_camber();
        let curves = development_curves(&camber);
        assert_eq!(curves.len(), camber.lines.len());
        for curve in &curves {
            let end = curve.last().unwrap();
            assert!(end[0].abs() < 1e-9 && end[1].abs() < 1e-9, "{end:?}");
            assert!(curve.iter().all(|p| p[0] <= 1e-9 && p[1] <= 1e-9));
        }
    }

    #[test]
    fn the_slope_is_the_true_angle() {
        let camber = b1_camber();
        for (curve, line) in development_curves(&camber).iter().zip(&camber.lines) {
            let slope = (curve[101][1] - curve[99][1]) / (curve[101][0] - curve[99][0]);
            let tan = line.points[100].beta.tan();
            assert!(
                ((slope - tan) / tan).abs() < 1e-2,
                "línea ψ = {}: pendiente {slope}, tan β {tan}",
                line.psi
            );
        }
    }

    #[test]
    fn the_most_forward_edge_is_at_minus_the_largest_wrap() {
        let camber = b1_camber();
        let first = development_curves(&camber)
            .iter()
            .map(|curve| curve[0][0])
            .fold(f64::INFINITY, f64::min);
        assert!(
            (first + camber.wrap_max.to_degrees()).abs() < 1e-9,
            "{first} frente a {}",
            camber.wrap_max.to_degrees()
        );
    }

    #[test]
    fn the_chart_defaults_to_beta() {
        assert_eq!(BladeChart::default(), BladeChart::Beta);
        assert_eq!(
            BladeChart::stored(&egui::Context::default()),
            BladeChart::Beta
        );
    }
}
