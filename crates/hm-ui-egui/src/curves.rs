//! Pestaña Curvas.

use eframe::egui;
use egui::Color32;
use egui_plot::{Corner, HLine, Legend, Line, Plot, PlotPoints, Points, Polygon, VLine};
use hm_engine::PREFERRED_OPERATING_REGION;

use crate::model::Outcome;

pub(super) fn line(name: &str, points: Vec<[f64; 2]>, color: Color32) -> Line<'static> {
    Line::new(name.to_string(), PlotPoints::from(points))
        .color(color)
        .width(2.0)
}

pub(super) fn curves(ui: &mut egui::Ui, outcome: &Outcome) {
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

    let palette = crate::theme::palette(ui.ctx());
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
            .fill_color(palette.preferred_region)
            .stroke(egui::Stroke::new(0.0, palette.ok)),
        );
        plot_ui.vline(VLine::new(name("Qd"), q_design).color(qd_color).width(1.5));
        plot_ui.vline(
            VLine::new(
                name("Inicio de recirculación"),
                outcome.result.recirculation_onset * q_design,
            )
            .color(palette.recirculation)
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
            plot_ui.line(line("H", head, palette.head));
            plot_ui.line(line("NPSHr", npshr, palette.npsh_r));
            if let Some(npsha) = &outcome.input.npsha {
                plot_ui.hline(HLine::new("NPSHa", npsha.si).color(palette.npsh_a));
            }
            plot_ui.points(
                Points::new("Punto de diseño", vec![[q_design, outcome.input.h.si]])
                    .radius(5.0)
                    .color(palette.head),
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
            plot_ui.line(line("η", eta, palette.efficiency));
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
            plot_ui.line(line("P eje", power, palette.power));
            if let Some(rating) = outcome
                .mechanics
                .as_ref()
                .ok()
                .and_then(|mech| mech.motor_rating)
            {
                plot_ui.hline(HLine::new("Motor IEC", rating / 1000.0).color(palette.motor_limit));
            }
        });
}
