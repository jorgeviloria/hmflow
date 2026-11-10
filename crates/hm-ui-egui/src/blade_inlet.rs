//! Lo que se elige del álabe antes de trazarlo, su forma y el número de líneas
//! de corriente, que va arriba del paso «Álabe β(r)», la tabla del triángulo
//! de entrada en cada línea, que va debajo de la gráfica, y lo que la capa
//! «Líneas de corriente» dibuja en el Meridiano.

use eframe::egui;
use hm_design::blade_setup::{BladeFamily, BladeSetup, BladeShape, STREAMLINE_COUNTS};
use hm_design::inlet_triangles::{
    AXIAL_BETA_2D_DEG, Blade2dAngle, INLET_MISMATCH_WARN_DEG, InletTriangle, inlet_triangles,
};
use hm_design::meridian::MeridionalModel;
use hm_design::streamlines::{Streamline, streamlines};

use crate::model::Outcome;

/// Cambia la forma solo si está disponible. Devuelve si cambió.
pub(super) fn choose_shape(setup: &mut BladeSetup, shape: BladeShape) -> bool {
    if !shape.is_available() || setup.shape == shape {
        return false;
    }
    setup.shape = shape;
    true
}

/// Cambia N solo si está en [`STREAMLINE_COUNTS`]. Devuelve si cambió.
pub(super) fn choose_streamlines(setup: &mut BladeSetup, count: u8) -> bool {
    if !STREAMLINE_COUNTS.contains(&count) || setup.streamlines == count {
        return false;
    }
    setup.streamlines = count;
    true
}

/// «Llega en el punto 2» o «Llega en el punto 4»; `None` si está disponible.
pub(super) fn arrival_hint(shape: BladeShape) -> Option<String> {
    shape
        .arrives_in()
        .map(|point| format!("Llega en el punto {point}"))
}

/// El texto del desplegable cerrado: «2D · Libre».
fn shape_label(shape: BladeShape) -> String {
    format!("{} · {}", shape.family().name(), shape.name())
}

/// El N de `HMF_STREAMLINES`, si es uno de [`STREAMLINE_COUNTS`].
pub(super) fn streamlines_from_text(text: &str) -> Option<u8> {
    text.trim()
        .parse()
        .ok()
        .filter(|count| STREAMLINE_COUNTS.contains(count))
}

/// El desplegable de la forma y el selector de N, en una fila. Devuelve si
/// cambió algo.
pub(super) fn blade_setup_controls(ui: &mut egui::Ui, setup: &mut BladeSetup) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        egui::ComboBox::from_label("Forma del álabe")
            .selected_text(shape_label(setup.shape))
            .show_ui(ui, |ui| {
                for (index, family) in [BladeFamily::ThreeD, BladeFamily::TwoD]
                    .into_iter()
                    .enumerate()
                {
                    if index > 0 {
                        ui.separator();
                    }
                    ui.label(egui::RichText::new(family.name()).small().weak());
                    for shape in BladeShape::ALL {
                        if shape.family() != family {
                            continue;
                        }
                        let selected = setup.shape == shape;
                        let response = ui
                            .add_enabled_ui(shape.is_available(), |ui| {
                                ui.selectable_label(selected, shape.name())
                            })
                            .inner;
                        let response = match arrival_hint(shape) {
                            Some(hint) => response.on_disabled_hover_text(hint),
                            None => response,
                        };
                        if response.clicked() {
                            changed |= choose_shape(setup, shape);
                        }
                    }
                }
            });
        ui.add_space(12.0);
        ui.label("Líneas de corriente");
        for count in STREAMLINE_COUNTS {
            if ui
                .selectable_label(setup.streamlines == count, count.to_string())
                .clicked()
            {
                changed |= choose_streamlines(setup, count);
            }
        }
    });
    changed
}

/// Holgura, en rad, con que un β 2D se considera el tope de un tramo axial.
const AXIAL_CAP_SLACK: f64 = 1.0e-12;

/// m → mm, para la columna r1.
const MM_PER_M: f64 = 1.0e3;

/// Las cabeceras de la tabla, en el orden de las columnas.
const INLET_HEADERS: [&str; 10] = [
    "Línea",
    "ψ",
    "r1 [mm]",
    "u1 [m/s]",
    "c1m [m/s]",
    "τ1",
    "β′1 [°]",
    "β1B [°]",
    "β 2D actual [°]",
    "Δβ [°]",
];

/// Una fila de la tabla «Entrada por línea de corriente», ya como texto.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct InletRow {
    /// «Cubo», «1», …, «Tapa».
    pub label: String,
    /// ψ, r1, u1, c1m, τ1, β′1 y β1B.
    pub cells: [String; 7],
    /// En grados, «≥ 89.9» o «sin álabe».
    pub beta_2d: String,
    /// El álabe 2D no llega al corte: `beta_2d` va con el color de aviso.
    pub no_blade: bool,
    /// Δβ con signo, o «—» sin álabe.
    pub mismatch: String,
    /// |Δβ| pasa de [`INLET_MISMATCH_WARN_DEG`].
    pub flagged: bool,
}

/// Los nombres de las filas, del cubo a la tapa.
pub(super) fn row_labels(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            if index == 0 {
                "Cubo".to_string()
            } else if index + 1 == count {
                "Tapa".to_string()
            } else {
                index.to_string()
            }
        })
        .collect()
}

/// La fila de un triángulo. Punto decimal, como la lectura de la gráfica.
fn row_of(label: String, triangle: &InletTriangle) -> InletRow {
    let beta_2d = match triangle.beta_2d {
        Blade2dAngle::NoBlade => "sin álabe".to_string(),
        Blade2dAngle::Angle(beta) if beta >= AXIAL_BETA_2D_DEG.to_radians() - AXIAL_CAP_SLACK => {
            format!("≥ {AXIAL_BETA_2D_DEG:.1}")
        }
        Blade2dAngle::Angle(beta) => format!("{:.1}", beta.to_degrees()),
    };
    let mismatch_deg = triangle.mismatch().map(f64::to_degrees);
    InletRow {
        label,
        cells: [
            format!("{:.2}", triangle.psi),
            format!("{:.1}", triangle.r1 * MM_PER_M),
            format!("{:.2}", triangle.u1),
            format!("{:.2}", triangle.c1m),
            format!("{:.3}", triangle.tau1),
            format!("{:.1}", triangle.beta1_flow.to_degrees()),
            format!("{:.1}", triangle.beta1b.to_degrees()),
        ],
        beta_2d,
        no_blade: triangle.beta_2d == Blade2dAngle::NoBlade,
        mismatch: mismatch_deg.map_or_else(|| "—".to_string(), |delta| format!("{delta:+.1}")),
        flagged: mismatch_deg.is_some_and(|delta| delta.abs() > INLET_MISMATCH_WARN_DEG),
    }
}

/// «Línea 2 · ψ = 0.50 · r1 = 51.3 mm · β1B = 24.6°»: lo que dice el corte de
/// una línea con el borde de ataque. El cubo y la tapa van con su nombre.
pub(super) fn cut_summary(label: &str, triangle: &InletTriangle) -> String {
    let name = if label.parse::<usize>().is_ok() {
        format!("Línea {label}")
    } else {
        label.to_string()
    };
    format!(
        "{name} · ψ = {:.2} · r1 = {:.1} mm · β1B = {:.1}°",
        triangle.psi,
        triangle.r1 * MM_PER_M,
        triangle.beta1b.to_degrees()
    )
}

/// Lo que la capa «Líneas de corriente» dibuja en el Meridiano, en mm.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct StreamlineLayer {
    /// Las líneas intermedias, `[z, r]`. Cubo y tapa ya son curvas del corte.
    pub inner: Vec<Vec<[f64; 2]>>,
    /// El corte de cada línea con el borde de ataque y su [`cut_summary`].
    pub cuts: Vec<([f64; 2], String)>,
}

/// La capa sobre el modelo que se dibuja; `None` si las líneas o sus
/// triángulos fallan (el mensaje ya está en la tabla del paso del álabe).
pub(super) fn streamline_layer(
    outcome: &Outcome,
    model: &MeridionalModel,
) -> Option<StreamlineLayer> {
    let (lines, triangles) = inlet_lines_of(outcome, model).ok()?;
    let last = lines.len().saturating_sub(1);
    let inner = lines
        .iter()
        .enumerate()
        .filter(|(index, _)| *index > 0 && *index < last)
        .map(|(_, line)| line.points.clone())
        .collect();
    let cuts = row_labels(lines.len())
        .iter()
        .zip(lines.iter().zip(&triangles))
        .map(|(label, (line, triangle))| (line.leading_edge, cut_summary(label, triangle)))
        .collect();
    Some(StreamlineLayer { inner, cuts })
}

/// El corte más cercano al puntero, en píxeles, si está a `tolerance` o menos.
pub(super) fn nearest_cut(cuts: &[[f32; 2]], pointer: [f32; 2], tolerance: f32) -> Option<usize> {
    cuts.iter()
        .map(|at| (at[0] - pointer[0]).hypot(at[1] - pointer[1]))
        .enumerate()
        .filter(|(_, distance)| *distance <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// Las líneas y sus triángulos sobre un modelo dado, o el mensaje del error.
fn inlet_lines_of(
    outcome: &Outcome,
    model: &MeridionalModel,
) -> Result<(Vec<Streamline>, Vec<InletTriangle>), String> {
    let geometry = outcome
        .geometry
        .as_ref()
        .map_err(std::string::ToString::to_string)?;
    let lines =
        streamlines(model, outcome.inputs.blade.streamlines).map_err(|error| error.to_string())?;
    let triangles = inlet_triangles(
        &outcome.result,
        &geometry.impeller,
        model,
        &lines,
        crate::blade_beta::applied_incidence_deg(&outcome.inputs),
    )
    .map_err(|error| error.to_string())?;
    Ok((lines, triangles))
}

/// Las líneas y sus triángulos de este diseño, o el mensaje que la tabla
/// enseña en su lugar. El meridiano es el editado; un recorte del cubo que
/// falle no cuenta, porque no cambia el perfil hidráulico.
pub(super) fn inlet_lines(
    outcome: &Outcome,
) -> Result<(Vec<Streamline>, Vec<InletTriangle>), String> {
    let edits = hm_engine::meridian_edits_of(&outcome.inputs.meridian);
    let (model, _) = crate::meridian::model_or_uncut(&outcome.result, &edits)
        .map_err(|error| error.to_string())?;
    inlet_lines_of(outcome, &model)
}

/// Las filas de la tabla, del cubo a la tapa, o el mensaje del error.
pub(super) fn inlet_rows(outcome: &Outcome) -> Result<Vec<InletRow>, String> {
    let (_, triangles) = inlet_lines(outcome)?;
    Ok(row_labels(triangles.len())
        .into_iter()
        .zip(&triangles)
        .map(|(label, triangle)| row_of(label, triangle))
        .collect())
}

/// La tabla «Entrada por línea de corriente», con su ayuda, bajo un título que
/// se pliega para devolver el sitio a la vista en planta. Si no se puede
/// calcular, el mensaje en su lugar.
pub(super) fn inlet_table(ui: &mut egui::Ui, outcome: &Outcome) {
    egui::CollapsingHeader::new(egui::RichText::new("Entrada por línea de corriente").strong())
        .default_open(true)
        .show(ui, |ui| match inlet_rows(outcome) {
            Ok(rows) => inlet_grid(ui, &rows),
            Err(message) => {
                let warn = crate::theme::palette(ui.ctx()).warn;
                ui.label(egui::RichText::new(message).color(warn));
            }
        });
}

/// Las filas, rayadas, y la ayuda debajo.
fn inlet_grid(ui: &mut egui::Ui, rows: &[InletRow]) {
    let warn = crate::theme::palette(ui.ctx()).warn;
    let tinted = |text: &str, tint: bool| {
        let text = egui::RichText::new(text);
        if tint { text.color(warn) } else { text }
    };
    egui::Grid::new("blade_inlet_table")
        .num_columns(INLET_HEADERS.len())
        .striped(true)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for header in INLET_HEADERS {
                ui.label(egui::RichText::new(header).small().strong());
            }
            ui.end_row();
            for row in rows {
                ui.label(&row.label);
                for cell in &row.cells {
                    ui.label(cell);
                }
                ui.label(tinted(&row.beta_2d, row.no_blade));
                ui.label(tinted(&row.mismatch, row.flagged));
                ui.end_row();
            }
        });
    ui.label(
        egui::RichText::new(
            "β1B: lo que pide el flujo en cada línea con el i1 del proyecto. β 2D actual: el \
             ángulo del álabe de hoy en ese punto; \"sin álabe\" si el corte queda por debajo \
             de d1/2, donde empieza el álabe 2D.",
        )
        .small()
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unavailable_shape_cannot_be_chosen() {
        for shape in BladeShape::ALL {
            if shape == BladeShape::Free2D {
                continue;
            }
            let mut setup = BladeSetup::default();
            assert!(!choose_shape(&mut setup, shape), "{shape:?}");
            assert_eq!(setup, BladeSetup::default(), "{shape:?}");
        }
    }

    #[test]
    fn an_available_shape_replaces_another() {
        let mut setup = BladeSetup {
            shape: BladeShape::Ruled3D,
            ..BladeSetup::default()
        };
        assert!(choose_shape(&mut setup, BladeShape::Free2D));
        assert_eq!(setup.shape, BladeShape::Free2D);
        // Elegir la que ya está no es un cambio.
        assert!(!choose_shape(&mut setup, BladeShape::Free2D));
    }

    #[test]
    fn only_the_four_counts_can_be_chosen() {
        for count in STREAMLINE_COUNTS {
            let mut setup = BladeSetup {
                streamlines: if count == 3 { 5 } else { 3 },
                ..BladeSetup::default()
            };
            assert!(choose_streamlines(&mut setup, count), "{count}");
            assert_eq!(setup.streamlines, count);
            assert!(!choose_streamlines(&mut setup, count), "{count} repetido");
        }
        for count in [4, 11, 0] {
            let mut setup = BladeSetup::default();
            assert!(!choose_streamlines(&mut setup, count), "{count}");
            assert_eq!(setup, BladeSetup::default(), "{count}");
        }
    }

    #[test]
    fn the_hint_names_the_point_where_the_shape_arrives() {
        assert_eq!(
            arrival_hint(BladeShape::Free3D).as_deref(),
            Some("Llega en el punto 2")
        );
        assert_eq!(
            arrival_hint(BladeShape::Circular2D).as_deref(),
            Some("Llega en el punto 4")
        );
        assert_eq!(arrival_hint(BladeShape::Free2D), None);
    }

    #[test]
    fn the_selected_text_carries_the_family_and_the_name() {
        assert_eq!(shape_label(BladeShape::Free2D), "2D · Libre");
        assert_eq!(shape_label(BladeShape::Ruled3D), "3D · Reglada");
    }

    #[test]
    fn the_capture_seed_only_takes_a_valid_count() {
        assert_eq!(streamlines_from_text(" 7 "), Some(7));
        assert_eq!(streamlines_from_text("4"), None);
        assert_eq!(streamlines_from_text("siete"), None);
    }

    use crate::model::{Basis, Inputs, compute};
    use hm_design::DesignError;
    use hm_design::meridian::meridional_model;

    fn outcome(streamlines: u8) -> Outcome {
        let mut inputs = Inputs::default();
        inputs.blade.streamlines = streamlines;
        compute(&inputs, &Basis::default()).unwrap()
    }

    /// Un triángulo con β1B = 20° y el β 2D que se pida.
    fn triangle(beta_2d: Blade2dAngle) -> InletTriangle {
        InletTriangle {
            psi: 0.5,
            r1: 0.0662,
            u1: 12.13,
            c1m: 3.138,
            tau1: 1.345,
            beta1_flow: 18.0_f64.to_radians(),
            beta1b: 20.0_f64.to_radians(),
            beta_2d,
        }
    }

    fn angle(deg: f64) -> Blade2dAngle {
        Blade2dAngle::Angle(deg.to_radians())
    }

    #[test]
    fn the_rows_are_named_from_hub_to_shroud() {
        assert_eq!(row_labels(5), ["Cubo", "1", "2", "3", "Tapa"]);
        assert_eq!(row_labels(3), ["Cubo", "1", "Tapa"]);
    }

    #[test]
    fn the_table_has_one_row_per_streamline() {
        for count in STREAMLINE_COUNTS {
            let rows = inlet_rows(&outcome(count)).unwrap();
            assert_eq!(rows.len(), usize::from(count));
            assert_eq!(rows[0].label, "Cubo");
            assert_eq!(rows[rows.len() - 1].label, "Tapa");
        }
    }

    #[test]
    fn a_row_shows_its_numbers_with_the_units_of_the_headers() {
        let row = row_of("2".into(), &triangle(angle(22.0)));
        assert_eq!(row.label, "2");
        assert_eq!(
            row.cells,
            ["0.50", "66.2", "12.13", "3.14", "1.345", "18.0", "20.0"]
        );
        assert_eq!(row.beta_2d, "22.0");
        assert_eq!(row.mismatch, "+2.0");
        assert!(!row.no_blade && !row.flagged);
    }

    #[test]
    fn a_row_without_a_2d_blade_says_so_and_has_no_mismatch() {
        let row = row_of("Cubo".into(), &triangle(Blade2dAngle::NoBlade));
        assert_eq!(row.beta_2d, "sin álabe");
        assert!(row.no_blade);
        assert_eq!(row.mismatch, "—");
        assert!(!row.flagged);
    }

    #[test]
    fn a_mismatch_over_four_degrees_is_flagged() {
        let over = row_of("1".into(), &triangle(angle(24.1)));
        assert_eq!(over.mismatch, "+4.1");
        assert!(over.flagged);
        let under = row_of("1".into(), &triangle(angle(15.9)));
        assert_eq!(under.mismatch, "-4.1");
        assert!(under.flagged);
        let within = row_of("1".into(), &triangle(angle(23.9)));
        assert_eq!(within.mismatch, "+3.9");
        assert!(!within.flagged);
    }

    #[test]
    fn an_axial_cut_shows_the_capped_angle() {
        let row = row_of("Tapa".into(), &triangle(angle(AXIAL_BETA_2D_DEG)));
        assert_eq!(row.beta_2d, "≥ 89.9");
        assert!(!row.no_blade);
    }

    // Review Focus 5: sin geometría no hay álabe 2D con el que comparar.
    #[test]
    fn a_design_without_geometry_shows_the_message_instead_of_rows() {
        let mut outcome = outcome(5);
        outcome.geometry = Err(DesignError::Input("no hay geometría".into()));
        let message = inlet_rows(&outcome).unwrap_err();
        assert!(message.contains("no hay geometría"), "{message}");
    }

    // Review Focus 1: un borde que no corta una línea intermedia.
    #[test]
    fn a_streamline_error_reaches_the_table_as_text() {
        let outcome = outcome(5);
        let mut model = meridional_model(&outcome.result).unwrap();
        let [shroud, hub] = model.params.edge_points;
        let stub = [
            shroud[0] + 0.05 * (hub[0] - shroud[0]),
            shroud[1] + 0.05 * (hub[1] - shroud[1]),
        ];
        let edge = model
            .curves
            .iter_mut()
            .find(|curve| curve.name == "Borde de ataque")
            .unwrap();
        edge.points = vec![shroud, stub];
        let message = inlet_lines_of(&outcome, &model).unwrap_err();
        assert!(message.contains("no corta el borde de ataque"), "{message}");
    }

    #[test]
    fn the_lines_and_the_triangles_come_in_pairs() {
        let outcome = outcome(7);
        let (lines, triangles) = inlet_lines(&outcome).unwrap();
        assert_eq!(lines.len(), 7);
        assert_eq!(triangles.len(), 7);
        for (line, triangle) in lines.iter().zip(&triangles) {
            assert_eq!(line.psi.to_bits(), triangle.psi.to_bits());
        }
    }

    #[test]
    fn the_summary_names_the_line_and_its_numbers() {
        let cut = InletTriangle {
            psi: 0.5,
            r1: 0.0513,
            beta1b: 24.6_f64.to_radians(),
            ..triangle(Blade2dAngle::NoBlade)
        };
        assert_eq!(
            cut_summary("2", &cut),
            "Línea 2 · ψ = 0.50 · r1 = 51.3 mm · β1B = 24.6°"
        );
    }

    #[test]
    fn the_summary_of_the_walls_uses_their_names() {
        let hub = InletTriangle {
            psi: 0.0,
            ..triangle(Blade2dAngle::NoBlade)
        };
        let shroud = InletTriangle {
            psi: 1.0,
            ..triangle(Blade2dAngle::NoBlade)
        };
        assert_eq!(
            cut_summary("Cubo", &hub),
            "Cubo · ψ = 0.00 · r1 = 66.2 mm · β1B = 20.0°"
        );
        assert_eq!(
            cut_summary("Tapa", &shroud),
            "Tapa · ψ = 1.00 · r1 = 66.2 mm · β1B = 20.0°"
        );
    }

    #[test]
    fn the_layer_draws_one_cut_per_streamline() {
        for count in STREAMLINE_COUNTS {
            let outcome = outcome(count);
            let model = meridional_model(&outcome.result).unwrap();
            let (lines, _) = inlet_lines(&outcome).unwrap();
            let layer = streamline_layer(&outcome, &model).unwrap();
            let count = usize::from(count);
            assert_eq!(lines.len(), count);
            assert_eq!(layer.cuts.len(), count);
            // Cubo y tapa ya son curvas del corte: solo se trazan las de dentro.
            assert_eq!(layer.inner.len(), count - 2);
            assert_eq!(layer.inner[0], lines[1].points);
            for (cut, line) in layer.cuts.iter().zip(&lines) {
                assert_eq!(cut.0, line.leading_edge);
            }
            assert!(layer.cuts[0].1.starts_with("Cubo · "));
            assert!(layer.cuts[1].1.starts_with("Línea 1 · "));
            assert!(layer.cuts[count - 1].1.starts_with("Tapa · "));
        }
    }

    #[test]
    fn a_design_without_lines_has_no_layer() {
        let mut outcome = outcome(5);
        let model = meridional_model(&outcome.result).unwrap();
        outcome.geometry = Err(DesignError::Input("no hay geometría".into()));
        assert!(streamline_layer(&outcome, &model).is_none());
    }

    #[test]
    fn the_pointer_picks_the_nearest_cut_within_reach() {
        let cuts = [[100.0, 100.0], [100.0, 120.0], [300.0, 300.0]];
        assert_eq!(nearest_cut(&cuts, [101.0, 112.0], 12.0), Some(1));
        assert_eq!(nearest_cut(&cuts, [101.0, 104.0], 12.0), Some(0));
        assert_eq!(nearest_cut(&cuts, [200.0, 200.0], 12.0), None);
        assert_eq!(nearest_cut(&[], [0.0, 0.0], 12.0), None);
    }
}
