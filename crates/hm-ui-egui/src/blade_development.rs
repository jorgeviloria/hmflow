//! Vistas del álabe 3D en el plano conforme (θ − Θ, m′), donde los ángulos son
//! los verdaderos: el desarrollo de las líneas medias y la vista «Perfil», el
//! álabe con su espesor, el vecino y la garganta de entrada.

use eframe::egui;
use egui_plot::{LineStyle, Plot, PlotPoint, Points, Polygon, Text};
use hm_design::blade_camber::{BladeCamber, CamberLine};
use hm_design::blade_solid::{BladeSolid, BladeSolidError};

use hm_design::blade_edges::{LeadingKind, cp_min};
use hm_design::blade_thickness::ThicknessWall;

use crate::blade_beta::{BetaHistory, GRAB_PIXELS};
use crate::blade_edges::{
    NoseHandle, TailHandle, drag_nose, drag_tail, nose_frames, nose_handles, tail_frames,
    tail_handles,
};
use crate::blade_thickness::ThicknessGraph;
use crate::curves::line;
use crate::model::{Inputs, Outcome};

/// Qué gráfica enseña el paso con forma 3D. Se guarda en los datos temporales
/// de egui.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum BladeChart {
    #[default]
    Beta,
    Thickness,
    Profile,
}

impl BladeChart {
    /// Las gráficas con su rótulo, en el orden del selector.
    pub(super) const ALL: [(Self, &'static str); 3] = [
        (Self::Beta, "β"),
        (Self::Thickness, "Espesor"),
        (Self::Profile, "Perfil"),
    ];

    /// Solo para las capturas: `HMF_CHART=beta|thickness|profile`.
    pub(super) fn from_env(text: Option<&str>) -> Option<Self> {
        match text {
            Some("thickness") => Some(Self::Thickness),
            Some("profile") => Some(Self::Profile),
            Some("beta") => Some(Self::Beta),
            Some(other) => {
                eprintln!("HMF_CHART: se espera beta, thickness o profile; llegó {other:?}");
                None
            }
            None => None,
        }
    }

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

/// El plano del dibujo de una línea: [(θ − Θ)°, (m′ − m′ de la salida)·180/π],
/// con Θ y m′ de la salida de esa línea. Todas acaban en y = 0.
pub(super) fn plane_of(line: &CamberLine) -> impl Fn(f64, f64) -> [f64; 2] {
    let (theta_end, m_end) = line
        .points
        .last()
        .map_or((0.0, 0.0), |p| (p.theta, p.m_conformal));
    move |theta, conformal| {
        [
            (theta - theta_end).to_degrees(),
            (conformal - m_end).to_degrees(),
        ]
    }
}

/// Una línea de corriente del perfil, en el plano del dibujo.
pub(super) struct ProfileLine {
    pub pressure: Vec<[f64; 2]>,
    pub suction: Vec<[f64; 2]>,
    /// La línea media, remuestreada como las dos caras.
    pub camber: Vec<[f64; 2]>,
    /// Las filas [presión, medio, succión] de la nariz, de la punta a la
    /// unión (la última es el primer punto de las caras). Vacío con romo.
    pub nose: Vec<[[f64; 2]; 3]>,
    /// Las filas [presión, medio, succión] de la salida con forma, de la
    /// unión (la última fila de las caras) a D2. Vacío con recortada.
    pub tail: Vec<[[f64; 2]; 3]>,
    /// Extremos de la garganta de entrada en el dibujo y su ancho en mm.
    pub throat: Option<([[f64; 2]; 2], f64)>,
    /// El trazo de a2′ (la distancia más corta al vecino con la forma de la
    /// salida), con a2′ y a2 en mm. Solo con una salida con forma.
    pub gap: Option<([[f64; 2]; 2], f64, f64)>,
}

/// Las caras, la línea media y la garganta de cada línea, en el plano del
/// dibujo ([`development_curves`] lo comparte).
pub(super) fn profile_lines(camber: &BladeCamber, solid: &BladeSolid) -> Vec<ProfileLine> {
    camber
        .lines
        .iter()
        .zip(&solid.lines)
        .zip(&solid.readings.throats)
        .zip(&solid.readings.outlet_gaps)
        .map(|(((camber_line, solid_line), throat), gap)| {
            let plane = plane_of(camber_line);
            let draw = |points: &[hm_design::blade_solid::SolidPoint]| -> Vec<[f64; 2]> {
                points
                    .iter()
                    .map(|p| plane(p.theta, p.m_conformal))
                    .collect()
            };
            ProfileLine {
                pressure: draw(&solid_line.pressure),
                suction: draw(&solid_line.suction),
                camber: draw(&solid_line.middle),
                nose: solid_line
                    .nose
                    .iter()
                    .map(|row| row.map(|p| plane(p.theta, p.m_conformal)))
                    .collect(),
                tail: solid_line
                    .tail
                    .iter()
                    .map(|row| row.map(|p| plane(p.theta, p.m_conformal)))
                    .collect(),
                gap: gap
                    .as_ref()
                    .filter(|_| !solid_line.tail.is_empty())
                    .map(|gap| {
                        (
                            [
                                plane(gap.from.theta, gap.from.m_conformal),
                                plane(gap.to.theta, gap.to.m_conformal),
                            ],
                            gap.shaped * 1000.0,
                            gap.cut * 1000.0,
                        )
                    }),
                throat: throat.as_ref().map(|throat| {
                    (
                        [
                            plane(throat.from.theta, throat.from.m_conformal),
                            plane(throat.to.theta, throat.to.m_conformal),
                        ],
                        throat.width * 1000.0,
                    )
                }),
            }
        })
        .collect()
}

/// El paso entre álabes, en grados: 360/Z.
pub(super) fn pitch_deg(blade_count: u32) -> f64 {
    360.0 / f64::from(blade_count)
}

/// Las marcas del dibujo: un punto con su texto por cada aviso y, si el sólido
/// falla donde se sabe (caras que se cruzan, vecinos que se tocan), en ese
/// punto.
pub(super) fn profile_marks(
    camber: &BladeCamber,
    solid: &Result<BladeSolid, BladeSolidError>,
) -> Vec<([f64; 2], String)> {
    let mark = |line: usize, at: [f64; 2], text: String| {
        camber
            .lines
            .get(line)
            .map(|camber_line| (plane_of(camber_line)(at[1], at[0]), text))
    };
    match solid {
        Ok(solid) => solid
            .notices
            .iter()
            .filter_map(|notice| mark(notice.line, notice.at, notice.message.clone()))
            .collect(),
        Err(
            error @ (BladeSolidError::FacesCross { line, at, .. }
            | BladeSolidError::NeighboursTouch { line, at, .. }),
        ) => mark(*line, *at, error.to_string()).into_iter().collect(),
        Err(_) => Vec::new(),
    }
}

/// Qué líneas enseña el perfil. Se guarda en los datos temporales de egui.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ProfileShown {
    pub hub: bool,
    pub shroud: bool,
    pub inner: bool,
}

impl Default for ProfileShown {
    /// Cubo y tapa.
    fn default() -> Self {
        Self {
            hub: true,
            shroud: true,
            inner: false,
        }
    }
}

impl ProfileShown {
    fn id() -> egui::Id {
        egui::Id::unique("blade_profile_shown")
    }

    fn stored(ctx: &egui::Context) -> Self {
        ctx.data(|data| data.get_temp::<Self>(Self::id()))
            .unwrap_or_default()
    }

    fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(Self::id(), self));
    }

    fn shows(self, index: usize, count: usize) -> bool {
        match index {
            0 => self.hub,
            i if i + 1 == count => self.shroud,
            _ => self.inner,
        }
    }
}

/// Las líneas medias como hoy, cuando el álabe con espesor no sale.
fn draw_camber_lines(plot_ui: &mut egui_plot::PlotUi, camber: &BladeCamber, shown: ProfileShown) {
    let palette = crate::theme::palette(plot_ui.ctx());
    let curves = development_curves(camber);
    let count = curves.len();
    for (index, curve) in curves.into_iter().enumerate() {
        if !shown.shows(index, count) {
            continue;
        }
        let (name, color) = match index {
            0 => ("Cubo", palette.hub),
            i if i + 1 == count => ("Tapa", palette.shroud),
            _ => ("Intermedias", palette.construction),
        };
        plot_ui.line(line(name, curve, color).width(2.0));
    }
}

/// El álabe relleno (un cuadrilátero por tramo, porque egui solo rellena
/// polígonos convexos), su contorno, la línea media a trazos, el vecino y la
/// garganta de cada línea que se enseña.
fn draw_solid(
    plot_ui: &mut egui_plot::PlotUi,
    lines: &[ProfileLine],
    shown: ProfileShown,
    pitch: f64,
) {
    let palette = crate::theme::palette(plot_ui.ctx());
    let count = lines.len();
    let shift = |points: &[[f64; 2]]| -> Vec<[f64; 2]> {
        points.iter().map(|p| [p[0] + pitch, p[1]]).collect()
    };
    for (index, line) in lines.iter().enumerate() {
        if !shown.shows(index, count) {
            continue;
        }
        let color = match index {
            0 => palette.hub,
            i if i + 1 == count => palette.shroud,
            _ => palette.construction,
        };
        for (p, s) in line.pressure.windows(2).zip(line.suction.windows(2)) {
            plot_ui.polygon(
                Polygon::new("", vec![p[0], p[1], s[1], s[0]])
                    .fill_color(color.gamma_multiply(0.35))
                    .stroke(egui::Stroke::NONE)
                    .allow_hover(false),
            );
        }
        // La nariz: relleno, contorno y la línea de unión (a trazos) con las
        // caras. Con romo, el canto recto de siempre.
        if line.nose.is_empty() {
            plot_ui.line(line_of(vec![line.pressure[0], line.suction[0]], color, 1.5));
        } else {
            for pair in line.nose.windows(2) {
                plot_ui.polygon(
                    Polygon::new("", vec![pair[0][0], pair[1][0], pair[1][2], pair[0][2]])
                        .fill_color(color.gamma_multiply(0.35))
                        .stroke(egui::Stroke::NONE)
                        .allow_hover(false),
                );
            }
            plot_ui.line(line_of(
                line.nose.iter().map(|row| row[0]).collect(),
                color,
                1.5,
            ));
            plot_ui.line(line_of(
                line.nose.iter().map(|row| row[2]).collect(),
                color,
                1.5,
            ));
            let tip = &line.nose[0];
            plot_ui.line(line_of(vec![tip[0], tip[2]], color, 1.5));
            plot_ui.line(
                line_of(vec![line.pressure[0], line.suction[0]], color, 1.0)
                    .style(LineStyle::dashed_dense()),
            );
        }
        for contour in [&line.pressure, &line.suction] {
            plot_ui.line(line_of(contour.clone(), color, 1.5));
        }
        let joint = vec![
            *line.pressure.last().unwrap(),
            *line.suction.last().unwrap(),
        ];
        if line.tail.is_empty() {
            plot_ui.line(line_of(joint, color, 1.5));
        } else {
            // La salida con forma: relleno, contorno, el canto de D2 y la
            // línea de unión (a trazos).
            for pair in line.tail.windows(2) {
                plot_ui.polygon(
                    Polygon::new("", vec![pair[0][0], pair[1][0], pair[1][2], pair[0][2]])
                        .fill_color(color.gamma_multiply(0.35))
                        .stroke(egui::Stroke::NONE)
                        .allow_hover(false),
                );
            }
            for side in [0, 2] {
                plot_ui.line(line_of(
                    line.tail.iter().map(|row| row[side]).collect(),
                    color,
                    1.5,
                ));
            }
            let exit = line.tail.last().unwrap();
            plot_ui.line(line_of(vec![exit[0], exit[2]], color, 1.5));
            plot_ui.line(line_of(joint, color, 1.0).style(LineStyle::dashed_dense()));
        }
        let camber_line: Vec<[f64; 2]> = line
            .nose
            .iter()
            .take(line.nose.len().saturating_sub(1))
            .map(|row| row[1])
            .chain(line.camber.iter().copied())
            .chain(line.tail.iter().skip(1).map(|row| row[1]))
            .collect();
        plot_ui.line(line_of(camber_line, color, 1.0).style(LineStyle::dashed_loose()));

        // El álabe vecino: el mismo contorno, +paso, sin relleno.
        let faint = color.gamma_multiply(0.6);
        for contour in [&line.pressure, &line.suction] {
            plot_ui.line(line_of(shift(contour), faint, 1.0));
        }
        if line.nose.is_empty() {
            plot_ui.line(line_of(
                shift(&[line.pressure[0], line.suction[0]]),
                faint,
                1.0,
            ));
        } else {
            for side in [0, 2] {
                let contour: Vec<[f64; 2]> = line.nose.iter().map(|row| row[side]).collect();
                plot_ui.line(line_of(shift(&contour), faint, 1.0));
            }
            let tip = &line.nose[0];
            plot_ui.line(line_of(shift(&[tip[0], tip[2]]), faint, 1.0));
        }
        if line.tail.is_empty() {
            plot_ui.line(line_of(
                shift(&[
                    *line.pressure.last().unwrap(),
                    *line.suction.last().unwrap(),
                ]),
                faint,
                1.0,
            ));
        } else {
            for side in [0, 2] {
                let contour: Vec<[f64; 2]> = line.tail.iter().map(|row| row[side]).collect();
                plot_ui.line(line_of(shift(&contour), faint, 1.0));
            }
            let exit = line.tail.last().unwrap();
            plot_ui.line(line_of(shift(&[exit[0], exit[2]]), faint, 1.0));
        }

        if let Some((ends, width)) = &line.throat {
            plot_ui.line(line_of(ends.to_vec(), palette.dimension, 1.5));
            let middle = PlotPoint::new(
                0.5 * (ends[0][0] + ends[1][0]),
                0.5 * (ends[0][1] + ends[1][1]),
            );
            plot_ui.text(
                Text::new(
                    "",
                    middle,
                    egui::RichText::new(format!(" {} mm", format!("{width:.1}").replace('.', ",")))
                        .small()
                        .color(palette.dimension_text),
                )
                .anchor(egui::Align2::LEFT_BOTTOM),
            );
        }
        // a2′ con la forma de la salida y, entre paréntesis, a2 recortada.
        if let Some((ends, shaped, cut)) = &line.gap {
            plot_ui.line(line_of(ends.to_vec(), palette.dimension, 1.5));
            let middle = PlotPoint::new(
                0.5 * (ends[0][0] + ends[1][0]),
                0.5 * (ends[0][1] + ends[1][1]),
            );
            plot_ui.text(
                Text::new(
                    "",
                    middle,
                    egui::RichText::new(format!(
                        " a2′ {} mm (a2 {})",
                        comma(*shaped, 1),
                        comma(*cut, 1)
                    ))
                    .small()
                    .color(palette.dimension_text),
                )
                // Cubo y tapa dan casi el mismo trazo: uno cuelga y el otro
                // se apoya.
                .anchor(if index == 0 {
                    egui::Align2::LEFT_TOP
                } else {
                    egui::Align2::LEFT_BOTTOM
                }),
            );
        }
    }
}

fn line_of(points: Vec<[f64; 2]>, color: egui::Color32, width: f32) -> egui_plot::Line<'static> {
    line("", points, color).width(width)
}

/// Un número con coma decimal.
fn comma(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

/// Cuánto se acerca un clic derecho a la nariz o a la salida para tomarlo como
/// «sobre ella», en píxeles.
const NOSE_MENU_PIXELS: f32 = 40.0;

/// Una cota que se agarra en el perfil: de la nariz o de la salida.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Handle {
    Nose(NoseHandle),
    Tail(TailHandle),
}

/// Zoom con la rueda, desplazamiento arrastrando y doble clic para reencuadrar.
/// Con el álabe con espesor, el vecino y la garganta; si no sale, las líneas
/// medias y el motivo bajo la vista. Con nariz, las cotas se agarran: Lp en la
/// línea de unión y, en el afilado, la punta y la asimetría; con salida con
/// forma, la longitud en su unión y, en el limado, el espesor final en D2. El
/// clic derecho sobre la nariz o la salida cambia el tipo. Lo que se mueve pasa
/// por el mismo `accepts` y el mismo historial que las filas «Entrada» y
/// «Salida». `true` si escribió `inputs`.
pub(super) fn profile_view(
    ui: &mut egui::Ui,
    outcome: &Outcome,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
    camber: &BladeCamber,
    solid: &Result<BladeSolid, BladeSolidError>,
    blade_count: u32,
) -> bool {
    let palette = crate::theme::palette(ui.ctx());
    let time = ui.input(|input| input.time);
    let mut shown = ProfileShown::stored(ui.ctx());
    ui.horizontal(|ui| {
        ui.checkbox(&mut shown.hub, "Cubo");
        ui.checkbox(&mut shown.shroud, "Tapa");
        ui.checkbox(&mut shown.inner, "Intermedias");
    });
    shown.store(ui.ctx());

    let lines = solid
        .as_ref()
        .ok()
        .map(|solid| profile_lines(camber, solid));
    let marks = profile_marks(camber, solid);
    let graph = ThicknessGraph::of(outcome, camber).ok();
    let same = inputs.blade.thickness.same;
    let handles: Vec<(Handle, [f64; 2])> = match solid {
        Ok(solid) => nose_handles(camber, solid, &inputs.blade.edges, same)
            .into_iter()
            .map(|(handle, at)| (Handle::Nose(handle), at))
            .chain(
                tail_handles(camber, solid, &inputs.blade.edges, same)
                    .into_iter()
                    .map(|(handle, at)| (Handle::Tail(handle), at)),
            )
            .collect(),
        Err(_) => Vec::new(),
    };
    let frames = graph.as_ref().and_then(|graph| {
        let e = [graph.curves.hub.at(0.0), graph.curves.shroud.at(0.0)];
        solid
            .as_ref()
            .ok()
            .and_then(|solid| nose_frames(camber, solid, e))
    });
    let tail_frames = graph.as_ref().and_then(|graph| {
        let e1 = [graph.curves.hub.at(1.0), graph.curves.shroud.at(1.0)];
        solid
            .as_ref()
            .ok()
            .and_then(|solid| tail_frames(camber, solid, e1))
    });
    let grabbed_id = egui::Id::unique("profile_nose_grabbed");
    let near_id = egui::Id::unique("profile_nose_near");
    let target_id = egui::Id::unique("profile_nose_target");
    let ctx = ui.ctx().clone();
    let grabbed: Option<Handle> = ctx.data(|data| data.get_temp(grabbed_id)).flatten();
    let near: bool = ctx.data(|data| data.get_temp(near_id)).unwrap_or(false);
    let mut written = false;
    let plot = Plot::new("blade_profile")
        .height(260.0)
        .data_aspect(1.0)
        // Con una manija bajo el puntero, arrastrar la mueve y no desplaza.
        .allow_drag(!(near || grabbed.is_some()))
        .allow_zoom(true)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(true)
        .x_axis_label("θ − θ salida [°]")
        .y_axis_label("m′ [°]")
        .show(ui, |plot_ui| {
            match &lines {
                Some(lines) => draw_solid(plot_ui, lines, shown, pitch_deg(blade_count)),
                None => draw_camber_lines(plot_ui, camber, shown),
            }
            for (at, text) in &marks {
                plot_ui.points(Points::new("", vec![*at]).color(palette.error).radius(5.0));
                plot_ui.text(
                    Text::new(
                        "",
                        PlotPoint::new(at[0], at[1]),
                        egui::RichText::new(format!(" {text}"))
                            .small()
                            .color(palette.error),
                    )
                    .anchor(egui::Align2::LEFT_BOTTOM),
                );
            }
            // Las manijas con su cota al lado.
            let edges = &inputs.blade.edges;
            for (handle, at) in &handles {
                let wall = match handle {
                    Handle::Nose(handle) => handle.wall(),
                    Handle::Tail(handle) => handle.wall(),
                };
                let hub = wall == ThicknessWall::Hub || same;
                let text = match handle {
                    Handle::Nose(handle) => {
                        let dims = if hub {
                            edges.leading_hub
                        } else {
                            edges.leading_shroud
                        };
                        match handle {
                            NoseHandle::Lp { .. } => {
                                let mut text = format!("Lp {} mm", comma(dims.lp * 1000.0, 1));
                                if let (Some(graph), true) =
                                    (&graph, edges.leading == LeadingKind::Ellipse)
                                {
                                    let e = graph.curves.hub.at(0.0);
                                    text.push_str(&format!(
                                        " · cp,min {}",
                                        comma(cp_min(e / dims.lp), 3)
                                    ));
                                }
                                text
                            }
                            NoseHandle::Tip { .. } => {
                                format!("punta {} mm", comma(dims.tip * 1000.0, 2))
                            }
                            NoseHandle::Asymmetry { .. } => {
                                format!("asimetría {}", comma(dims.asymmetry, 2))
                            }
                        }
                    }
                    Handle::Tail(handle) => {
                        let dims = if hub {
                            edges.trailing_hub
                        } else {
                            edges.trailing_shroud
                        };
                        match handle {
                            TailHandle::Length { .. } => {
                                format!("longitud {} mm", comma(dims.length * 1000.0, 1))
                            }
                            TailHandle::End { .. } => {
                                format!("espesor final {} mm", comma(dims.end * 1000.0, 2))
                            }
                        }
                    }
                };
                let grabbed_now = grabbed == Some(*handle);
                plot_ui.points(
                    Points::new("", vec![*at])
                        .color(palette.dimension)
                        .radius(if grabbed_now { 8.0 } else { 6.0 })
                        .filled(true),
                );
                plot_ui.text(
                    Text::new(
                        "",
                        PlotPoint::new(at[0], at[1]),
                        egui::RichText::new(format!(" {text}"))
                            .small()
                            .color(palette.dimension_text),
                    )
                    // La punta y la asimetría casi coinciden: una cuelga debajo
                    // de su manija y la otra se apoya encima. En la salida, la longitud
                    // sube a la izquierda y el espesor final baja a la derecha.
                    .anchor(match handle {
                        Handle::Nose(NoseHandle::Asymmetry { .. }) => egui::Align2::RIGHT_BOTTOM,
                        Handle::Tail(TailHandle::Length { .. }) => egui::Align2::RIGHT_BOTTOM,
                        Handle::Tail(TailHandle::End { .. }) => egui::Align2::LEFT_TOP,
                        Handle::Nose(_) => egui::Align2::LEFT_TOP,
                    }),
                );
            }
            // El arrastre y lo que hay bajo el puntero.
            let response = plot_ui.response().clone();
            let pointer = plot_ui.pointer_coordinate();
            let to_screen = |p: [f64; 2]| plot_ui.screen_from_plot(PlotPoint::new(p[0], p[1]));
            if let Some(pointer) = pointer {
                let at = plot_ui.screen_from_plot(pointer);
                let nearest = handles
                    .iter()
                    .map(|(handle, point)| (*handle, to_screen(*point).distance(at)))
                    .filter(|(_, distance)| *distance <= GRAB_PIXELS)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(handle, _)| handle);
                ctx.data_mut(|data| data.insert_temp(near_id, nearest.is_some()));
                if response.drag_started() {
                    ctx.data_mut(|data| data.insert_temp(grabbed_id, nearest));
                }
                if response.secondary_clicked() {
                    let close = |point: &[f64; 2]| to_screen(*point).distance(at) <= NOSE_MENU_PIXELS;
                    let on_nose = lines
                        .iter()
                        .flatten()
                        .any(|line| line.nose.iter().flatten().any(close));
                    // La salida: su forma o, recortada, el canto de D2.
                    let on_tail = lines.iter().flatten().any(|line| {
                        line.tail.iter().flatten().any(close)
                            || close(line.pressure.last().unwrap())
                            || close(line.suction.last().unwrap())
                    });
                    ctx.data_mut(|data| data.insert_temp(target_id, (on_nose, on_tail)));
                }
                if let (Some(handle), true, Some(graph)) =
                    (grabbed, response.dragged(), graph.as_ref())
                {
                    let pointer = [pointer.x, pointer.y];
                    let moved = match handle {
                        Handle::Nose(handle) => frames.as_ref().map(|frames| {
                            drag_nose(handle, pointer, &inputs.blade.edges, frames, same)
                        }),
                        Handle::Tail(handle) => tail_frames.as_ref().map(|frames| {
                            drag_tail(handle, pointer, &inputs.blade.edges, frames, same)
                        }),
                    };
                    if let Some(edges) = moved {
                        let status: fn(&egui::Context, Option<String>) = match handle {
                            Handle::Nose(_) => crate::blade_edges::set_status,
                            Handle::Tail(_) => crate::blade_edges::set_trailing_status,
                        };
                        let mut candidate = inputs.blade.clone();
                        candidate.edges = edges;
                        match crate::blade_edges::commit(
                            outcome, inputs, history, graph, candidate, time,
                        ) {
                            Ok(changed) => {
                                written |= changed;
                                status(&ctx, None);
                            }
                            Err(reason) => {
                                status(&ctx, Some(format!("No se mueve: {reason}")));
                            }
                        }
                    }
                }
            }
            if response.drag_stopped() {
                ctx.data_mut(|data| data.remove::<Option<Handle>>(grabbed_id));
            }
        });
    // Clic derecho sobre la nariz o la salida: el tipo del borde.
    let (on_nose, on_tail): (bool, bool) = ctx
        .data(|data| data.get_temp(target_id))
        .unwrap_or((false, false));
    plot.response.context_menu(|ui| {
        if !on_nose && !on_tail {
            ui.label(
                egui::RichText::new("Clic derecho sobre la nariz o la salida.")
                    .small()
                    .weak(),
            );
            return;
        }
        if on_nose {
            ui.label(egui::RichText::new("Entrada").small().weak());
            let same = inputs.blade.thickness.same;
            for (choice, label) in crate::blade_edges::LEADING_CHOICES {
                let (selected, e_hub, e_shroud) = match &graph {
                    Some(graph) => {
                        let (e_hub, e_shroud) =
                            (graph.curves.hub.at(0.0), graph.curves.shroud.at(0.0));
                        (
                            crate::blade_edges::choice_selected(
                                choice,
                                &inputs.blade.edges,
                                same,
                                e_hub,
                                e_shroud,
                            ),
                            e_hub,
                            e_shroud,
                        )
                    }
                    None => (false, 0.0, 0.0),
                };
                if ui.selectable_label(selected, label).clicked() {
                    if let Some(graph) = &graph {
                        let candidate = crate::blade_edges::choose_leading(
                            &inputs.blade,
                            choice,
                            e_hub,
                            e_shroud,
                        );
                        match crate::blade_edges::commit(
                            outcome, inputs, history, graph, candidate, time,
                        ) {
                            Ok(changed) => written |= changed,
                            Err(reason) => crate::blade_edges::set_status(
                                ui.ctx(),
                                Some(format!("No se mueve: {reason}")),
                            ),
                        }
                    }
                    ui.close();
                }
            }
        }
        if on_nose && on_tail {
            ui.separator();
        }
        if on_tail {
            ui.label(egui::RichText::new("Salida").small().weak());
            let current = inputs.blade.edges.trailing;
            for (kind, label) in crate::blade_edges::TRAILING_KINDS {
                if ui.selectable_label(kind == current, label).clicked() {
                    if let Some(graph) = &graph {
                        let (e1_hub, e1_shroud) =
                            (graph.curves.hub.at(1.0), graph.curves.shroud.at(1.0));
                        let candidate = crate::blade_edges::with_trailing_kind(
                            &inputs.blade,
                            kind,
                            e1_hub,
                            e1_shroud,
                        );
                        match crate::blade_edges::commit(
                            outcome, inputs, history, graph, candidate, time,
                        ) {
                            Ok(changed) => written |= changed,
                            Err(reason) => crate::blade_edges::set_trailing_status(
                                ui.ctx(),
                                Some(format!("No se mueve: {reason}")),
                            ),
                        }
                    }
                    ui.close();
                }
            }
        }
    });
    if let Err(error) = solid {
        ui.label(
            egui::RichText::new(format!("Sin álabe con espesor: {error}."))
                .small()
                .color(palette.error),
        );
    }
    ui.label(
        egui::RichText::new(
            "Perfil: el álabe con su espesor en el plano conforme, donde los ángulos son los \
             verdaderos. A trazos, la línea media. Con nariz, arrastra Lp en la línea de unión y, \
             en el afilado, la punta y la asimetría; con la salida con forma, su longitud y, en \
             el limado, el espesor final. Clic derecho sobre la nariz o la salida cambia el tipo.",
        )
        .small()
        .weak(),
    );
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Basis;
    use crate::model::Inputs;
    use hm_design::blade_setup::BladeShape;
    use hm_design::blade_solid::BladeSolidError;

    #[test]
    fn the_capture_variable_picks_the_chart() {
        assert_eq!(
            BladeChart::from_env(Some("thickness")),
            Some(BladeChart::Thickness)
        );
        assert_eq!(
            BladeChart::from_env(Some("profile")),
            Some(BladeChart::Profile)
        );
        assert_eq!(BladeChart::from_env(Some("nada")), None);
        assert_eq!(BladeChart::from_env(None), None);
    }

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

    /// B1 con forma libre 3D y cinco líneas de corriente: la línea media y el
    /// álabe con espesor.
    fn b1_outcome() -> Outcome {
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free3D;
        inputs.blade.streamlines = 5;
        crate::model::compute(&inputs, &Basis::default()).unwrap()
    }

    fn b1_solid() -> (BladeCamber, BladeSolid, u32) {
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free3D;
        inputs.blade.streamlines = 5;
        let outcome = crate::model::compute(&inputs, &Basis::default()).unwrap();
        let camber = crate::blade_inlet::camber_of(&outcome)
            .unwrap()
            .expect("forma 3D");
        let solid = crate::blade_inlet::solid_of(&outcome).unwrap();
        (camber, solid, outcome.result.selection.blade_count)
    }

    #[test]
    fn both_faces_end_on_the_exit_line() {
        let (camber, solid, _) = b1_solid();
        let lines = profile_lines(&camber, &solid);
        assert_eq!(lines.len(), camber.lines.len());
        for line in &lines {
            for face in [&line.pressure, &line.suction, &line.camber] {
                assert!(face.last().unwrap()[1].abs() < 1e-9, "{face:?}");
            }
        }
    }

    #[test]
    fn the_camber_line_runs_between_the_faces() {
        let (camber, solid, _) = b1_solid();
        for line in profile_lines(&camber, &solid) {
            let half = line.pressure.len() / 2;
            assert!(line.pressure[half][0] < line.camber[half][0]);
            assert!(line.camber[half][0] < line.suction[half][0]);
        }
    }

    #[test]
    fn the_pitch_is_360_over_z() {
        assert!((pitch_deg(6) - 60.0).abs() < 1e-12);
        assert!((pitch_deg(1) - 360.0).abs() < 1e-12);
    }

    #[test]
    fn the_throat_joins_the_neighbour_nose_and_the_suction_face() {
        let (camber, solid, z) = b1_solid();
        let lines = profile_lines(&camber, &solid);
        for (index, line) in lines.iter().enumerate() {
            let (ends, width) = line.throat.expect("Z > 1 tiene garganta");
            let throat = solid.readings.throats[index].as_ref().unwrap();
            let plane = plane_of(&camber.lines[index]);
            assert_eq!(ends[0], plane(throat.from.theta, throat.from.m_conformal));
            assert_eq!(ends[1], plane(throat.to.theta, throat.to.m_conformal));
            assert!((width - 1000.0 * throat.width).abs() < 1e-9);
            // El extremo del vecino ya lleva el paso: es la nariz de este
            // álabe, un paso más allá y a la misma m′.
            let nose = line.pressure[0];
            assert!((ends[0][0] - nose[0] - pitch_deg(z)).abs() < 1e-9);
            assert!((ends[0][1] - nose[1]).abs() < 1e-9);
        }
    }

    #[test]
    fn an_error_is_marked_where_it_happens() {
        let (camber, _, _) = b1_solid();
        let error = BladeSolidError::FacesCross {
            line: 1,
            radius: 0.05,
            at: [0.3, 1.2],
        };
        let marks = profile_marks(&camber, &Err(error.clone()));
        assert_eq!(marks.len(), 1);
        let plane = plane_of(&camber.lines[1]);
        assert_eq!(marks[0].0, plane(1.2, 0.3));
        assert_eq!(marks[0].1, error.to_string());
        // Un error sin lugar no marca nada.
        assert!(profile_marks(&camber, &Err(BladeSolidError::LineMismatch)).is_empty());
    }

    #[test]
    fn notices_are_marked_too() {
        let (camber, mut solid, _) = b1_solid();
        solid.notices.push(hm_design::blade_solid::SolidNotice {
            line: 2,
            at: [0.4, 0.9],
            message: "poco espesor".into(),
        });
        let marks = profile_marks(&camber, &Ok(solid));
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].0, plane_of(&camber.lines[2])(0.9, 0.4));
        assert_eq!(marks[0].1, "poco espesor");
    }

    #[test]
    fn the_selector_says_profile() {
        let labels: Vec<&str> = BladeChart::ALL.iter().map(|(_, label)| *label).collect();
        assert_eq!(labels, ["β", "Espesor", "Perfil"]);
        assert_eq!(BladeChart::ALL[2].0, BladeChart::Profile);
    }

    #[test]
    fn the_view_draws_with_a_solid_and_with_an_error() {
        let (camber, solid, z) = b1_solid();
        let marked = BladeSolidError::NeighboursTouch {
            line: 0,
            radius: 0.07,
            at: [0.1, 0.2],
        };
        for result in [Ok(solid), Err(marked)] {
            let outcome = b1_outcome();
            let mut inputs = outcome.inputs.clone();
            let mut history = BetaHistory::default();
            egui::__run_test_ui(|ui| {
                profile_view(ui, &outcome, &mut inputs, &mut history, &camber, &result, z);
            });
        }
    }

    #[test]
    fn the_profile_view_draws_the_nose_without_panicking() {
        use hm_design::blade_edges::LeadingKind;
        for kind in [LeadingKind::Blunt, LeadingKind::Ellipse, LeadingKind::Wedge] {
            let mut inputs = Inputs::default();
            inputs.blade.shape = BladeShape::Free3D;
            inputs.blade.edges = inputs.blade.edges.with_leading_kind(kind, 0.0077, 0.0077);
            let outcome = crate::model::compute(&inputs, &Basis::default()).unwrap();
            let camber = crate::blade_inlet::camber_of(&outcome)
                .unwrap()
                .expect("forma 3D");
            let solid = crate::blade_inlet::solid_of(&outcome);
            assert!(solid.is_ok(), "{kind:?}: {solid:?}");
            let mut history = BetaHistory::default();
            for same in [true, false] {
                let mut inputs = outcome.inputs.clone();
                if !same {
                    inputs.blade = crate::blade_edges::switch_same(
                        &inputs.blade,
                        &crate::blade_thickness::ThicknessGraph::of(&outcome, &camber)
                            .unwrap()
                            .curves,
                        false,
                    );
                }
                egui::__run_test_ui(|ui| {
                    profile_view(
                        ui,
                        &outcome,
                        &mut inputs,
                        &mut history,
                        &camber,
                        &solid,
                        outcome.result.selection.blade_count,
                    );
                });
            }
        }
    }

    #[test]
    fn a_2d_shape_has_no_solid_and_says_why() {
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free2D;
        let outcome = crate::model::compute(&inputs, &Basis::default()).unwrap();
        assert!(matches!(
            crate::blade_inlet::solid_of(&outcome),
            Err(BladeSolidError::NoCamber(_))
        ));
    }
    #[test]
    fn the_profile_lines_carry_the_tail_and_the_gap_of_a_shaped_exit() {
        use hm_design::blade_edges::TrailingKind;
        for kind in [
            TrailingKind::Cut,
            TrailingKind::FiledSuction,
            TrailingKind::FiledPressure,
            TrailingKind::Ellipse,
        ] {
            let mut inputs = Inputs::default();
            inputs.blade.shape = BladeShape::Free3D;
            inputs.blade.streamlines = 5;
            inputs.blade.edges = inputs.blade.edges.with_trailing_kind(kind, 0.0077, 0.0077);
            let outcome = crate::model::compute(&inputs, &Basis::default()).unwrap();
            let camber = crate::blade_inlet::camber_of(&outcome).unwrap().unwrap();
            let solid = crate::blade_inlet::solid_of(&outcome).unwrap();
            let lines = profile_lines(&camber, &solid);
            for (index, line) in lines.iter().enumerate() {
                let plane = plane_of(&camber.lines[index]);
                let tail = &solid.lines[index].tail;
                assert_eq!(line.tail.len(), tail.len(), "{kind:?}");
                if kind == TrailingKind::Cut {
                    assert!(line.gap.is_none(), "la recortada no dibuja a2′");
                    continue;
                }
                // La última fila del trazo es el canto de D2, sobre y = 0.
                let last = line.tail.last().unwrap();
                assert!(last[1][1].abs() < 1e-9, "{last:?}");
                // El trazo de a2′ une los puntos de la lectura, con sus cotas.
                let gap = solid.readings.outlet_gaps[index].as_ref().unwrap();
                let (ends, shaped, cut) = line.gap.expect("con forma hay a2′");
                assert_eq!(ends[0], plane(gap.from.theta, gap.from.m_conformal));
                assert_eq!(ends[1], plane(gap.to.theta, gap.to.m_conformal));
                assert!((shaped - 1000.0 * gap.shaped).abs() < 1e-9);
                assert!((cut - 1000.0 * gap.cut).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn the_profile_view_draws_the_tail_and_the_gaps_without_panicking() {
        use hm_design::blade_edges::TrailingKind;
        for kind in [
            TrailingKind::Cut,
            TrailingKind::FiledSuction,
            TrailingKind::FiledPressure,
            TrailingKind::Ellipse,
        ] {
            let mut inputs = Inputs::default();
            inputs.blade.shape = BladeShape::Free3D;
            inputs.blade.edges = inputs.blade.edges.with_trailing_kind(kind, 0.0077, 0.0077);
            let outcome = crate::model::compute(&inputs, &Basis::default()).unwrap();
            let camber = crate::blade_inlet::camber_of(&outcome)
                .unwrap()
                .expect("forma 3D");
            let solid = crate::blade_inlet::solid_of(&outcome);
            assert!(solid.is_ok(), "{kind:?}: {solid:?}");
            let mut history = BetaHistory::default();
            for same in [true, false] {
                let mut inputs = outcome.inputs.clone();
                if !same {
                    inputs.blade = crate::blade_edges::switch_same(
                        &inputs.blade,
                        &crate::blade_thickness::ThicknessGraph::of(&outcome, &camber)
                            .unwrap()
                            .curves,
                        false,
                    );
                }
                egui::__run_test_ui(|ui| {
                    profile_view(
                        ui,
                        &outcome,
                        &mut inputs,
                        &mut history,
                        &camber,
                        &solid,
                        outcome.result.selection.blade_count,
                    );
                });
            }
        }
    }
}
