//! Lo que se elige del álabe antes de trazarlo, su forma y el número de líneas
//! de corriente, que va arriba del paso «Álabe β(r)», la tabla del triángulo
//! de entrada en cada línea, que va debajo de la gráfica, y lo que la capa
//! «Líneas de corriente» dibuja en el Meridiano.

use eframe::egui;
use hm_design::blade_camber::{BladeCamber, CamberOrigin, camber_for_design, wrap_range_deg};
use hm_design::blade_setup::{BladeFamily, BladeSetup, BladeShape, STREAMLINE_COUNTS};
use hm_design::blade_solid::{BladeSolid, BladeSolidError, blade_solid_for_design};
use hm_design::inlet_triangles::{
    Blade2dAngle, INLET_MISMATCH_WARN_DEG, InletTriangle, inlet_triangles,
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
    /// En grados, «sin álabe» o «tramo axial».
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
        Blade2dAngle::Axial => "tramo axial".to_string(),
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

/// El borde de ataque de la reglada sobre el Meridiano, en mm.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RuledEdgeLayer {
    /// `[z, r]`: los nodos 0 de la línea media, del cubo a la tapa.
    pub edge: Vec<[f64; 2]>,
    /// El mayor `|edge_gap|`, mm.
    pub max_gap_mm: f64,
}

/// La capa sobre el modelo que se dibuja; `None` si la forma no es la
/// reglada o la línea media falla (el mensaje ya está en el paso del álabe).
pub(super) fn ruled_edge_layer(
    outcome: &Outcome,
    model: &MeridionalModel,
) -> Option<RuledEdgeLayer> {
    let setup = &outcome.inputs.blade;
    if setup.shape != BladeShape::Ruled3D {
        return None;
    }
    let geometry = outcome.geometry.as_ref().ok()?;
    let camber = camber_for_design(
        &outcome.result,
        &geometry.impeller,
        model,
        setup,
        crate::blade_beta::applied_incidence_deg(&outcome.inputs),
    )
    .ok()??;
    let edge = camber
        .lines
        .iter()
        .map(|line| {
            let node = line.points.first()?;
            Some([node.z * MM_PER_M, node.r * MM_PER_M])
        })
        .collect::<Option<Vec<_>>>()?;
    let max_gap_mm = camber
        .lines
        .iter()
        .map(|line| line.edge_gap.abs() * MM_PER_M)
        .fold(0.0, f64::max);
    Some(RuledEdgeLayer { edge, max_gap_mm })
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

/// Las cabeceras de la tabla «Álabe por línea de corriente».
const BLADE_HEADERS: [&str; 13] = [
    "Línea",
    "ψ",
    "r1 [mm]",
    "u1 [m/s]",
    "c1m [m/s]",
    "τ1",
    "β′1 [°]",
    "β1B [°]",
    "β álabe [°]",
    "Δβ [°]",
    "ε [°]",
    "θ borde [°]",
    "L [mm]",
];

/// Una fila de la tabla «Álabe por línea de corriente», ya como texto.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct BladeRow {
    /// «Cubo», «1», …, «Tapa».
    pub label: String,
    /// ψ, r1, u1, c1m, τ1, β′1, β1B, β álabe, Δβ, ε, θ borde y L.
    pub cells: [String; 12],
    /// |Δβ| pasa de [`INLET_MISMATCH_WARN_DEG`]: Δβ va con el color de aviso.
    pub flagged: bool,
}

/// Un ángulo con signo y un decimal; un cero (también el redondeado) sin signo.
fn signed_degrees(value: f64) -> String {
    let text = format!("{value:+.1}");
    if text == "+0.0" || text == "-0.0" {
        "0.0".to_string()
    } else {
        text
    }
}

/// La línea media 3D de este diseño sobre el meridiano editado; `Ok(None)` con
/// forma 2D, y el mensaje del error si falla.
pub(super) fn camber_of(outcome: &Outcome) -> Result<Option<BladeCamber>, String> {
    let setup = &outcome.inputs.blade;
    if setup.shape.family() != BladeFamily::ThreeD {
        return Ok(None);
    }
    let geometry = outcome
        .geometry
        .as_ref()
        .map_err(std::string::ToString::to_string)?;
    let edits = hm_engine::meridian_edits_of(&outcome.inputs.meridian);
    let (model, _) = crate::meridian::model_or_uncut(&outcome.result, &edits)
        .map_err(|error| error.to_string())?;
    camber_for_design(
        &outcome.result,
        &geometry.impeller,
        &model,
        setup,
        crate::blade_beta::applied_incidence_deg(&outcome.inputs),
    )
    .map_err(|error| error.to_string())
}

/// El álabe con espesor de este diseño, por el mismo camino que
/// [`camber_of`]: sobre el meridiano editado y con la incidencia aplicada.
/// Con forma 2D, o si algo falla, el error dice por qué.
pub(super) fn solid_of(outcome: &Outcome) -> Result<BladeSolid, BladeSolidError> {
    solid_for(outcome, &outcome.inputs.blade)
}

/// Como [`solid_of`], con otra elección de álabe sobre el mismo diseño: para
/// saber si un cambio de espesor se podría construir antes de escribirlo.
pub(super) fn solid_for(
    outcome: &Outcome,
    setup: &BladeSetup,
) -> Result<BladeSolid, BladeSolidError> {
    let no_solid = |text: String| BladeSolidError::NoCamber(text);
    let geometry = outcome
        .geometry
        .as_ref()
        .map_err(|error| no_solid(error.to_string()))?;
    let edits = hm_engine::meridian_edits_of(&outcome.inputs.meridian);
    let (model, _) = crate::meridian::model_or_uncut(&outcome.result, &edits)
        .map_err(|error| no_solid(error.to_string()))?;
    blade_solid_for_design(
        &outcome.result,
        &geometry.impeller,
        &model,
        setup,
        crate::blade_beta::applied_incidence_deg(&outcome.inputs),
    )
}

/// Las filas de la tabla 3D, del cubo a la tapa, o el mensaje del error.
pub(super) fn blade_rows(outcome: &Outcome) -> Result<Vec<BladeRow>, String> {
    let (_, triangles) = inlet_lines(outcome)?;
    let camber = camber_of(outcome)?
        .ok_or_else(|| "la forma del álabe no es 3D: no hay línea media por línea".to_string())?;
    Ok(row_labels(triangles.len())
        .into_iter()
        .zip(triangles.iter().zip(&camber.lines))
        .map(|(label, (triangle, line))| {
            let inlet = row_of(label.clone(), triangle);
            let first = &line.points[0];
            let delta = (first.beta - triangle.beta1b).to_degrees();
            let [psi, r1, u1, c1m, tau1, beta_flow, beta1b] = inlet.cells;
            BladeRow {
                label,
                cells: [
                    psi,
                    r1,
                    u1,
                    c1m,
                    tau1,
                    beta_flow,
                    beta1b,
                    format!("{:.1}", first.beta.to_degrees()),
                    signed_degrees(delta),
                    format!("{:.1}", line.wrap.to_degrees()),
                    format!("{:.1}", first.theta.to_degrees()),
                    format!("{:.1}", line.length * MM_PER_M),
                ],
                flagged: delta.abs() > INLET_MISMATCH_WARN_DEG,
            }
        })
        .collect())
}

/// «Gülich (p. 422): con Z = 5, envolvimiento de 130 a 160°.»; `None` si
/// Gülich no da rango para ese Z.
fn wrap_reference(blade_count: u32) -> Option<String> {
    wrap_range_deg(blade_count).map(|(low, high)| {
        format!("Gülich (p. 422): con Z = {blade_count}, envolvimiento de {low:.0} a {high:.0}°.")
    })
}

/// El mayor desvío del borde de ataque de la reglada, en mm; `None` si el
/// álabe no es reglado.
fn max_edge_gap_mm(camber: &BladeCamber) -> Option<f64> {
    camber
        .lines
        .iter()
        .filter(|line| line.origin == CamberOrigin::Ruled)
        .map(|line| line.edge_gap.abs() * MM_PER_M)
        .max_by(f64::total_cmp)
}

/// La tabla del paso del álabe, con su ayuda, bajo un título que se pliega
/// para devolver el sitio a la vista en planta: «Álabe por línea de corriente»
/// con una forma 3D y «Entrada por línea de corriente» con una 2D. Si no se
/// puede calcular, el mensaje en su lugar.
pub(super) fn inlet_table(ui: &mut egui::Ui, outcome: &Outcome) {
    let three_d = outcome.inputs.blade.shape.family() == BladeFamily::ThreeD;
    let title = if three_d {
        "Álabe por línea de corriente"
    } else {
        "Entrada por línea de corriente"
    };
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .default_open(true)
        .show(ui, |ui| {
            let warn = crate::theme::palette(ui.ctx()).warn;
            let shown = if three_d {
                blade_rows(outcome).map(|rows| blade_grid(ui, &rows, outcome))
            } else {
                inlet_rows(outcome).map(|rows| inlet_grid(ui, &rows))
            };
            if let Err(message) = shown {
                ui.label(egui::RichText::new(message).color(warn));
            }
        });
}

/// Las filas de la tabla 3D, rayadas, y la ayuda debajo.
fn blade_grid(ui: &mut egui::Ui, rows: &[BladeRow], outcome: &Outcome) {
    let warn = crate::theme::palette(ui.ctx()).warn;
    egui::Grid::new("blade_camber_table")
        .num_columns(BLADE_HEADERS.len())
        .striped(true)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for header in BLADE_HEADERS {
                ui.label(egui::RichText::new(header).small().strong());
            }
            ui.end_row();
            for row in rows {
                ui.label(&row.label);
                for (index, cell) in row.cells.iter().enumerate() {
                    let text = egui::RichText::new(cell);
                    // Δβ es la novena celda.
                    ui.label(if index == 8 && row.flagged {
                        text.color(warn)
                    } else {
                        text
                    });
                }
                ui.end_row();
            }
        });
    let mut small = |text: String| ui.label(egui::RichText::new(text).small().weak());
    small(
        "β álabe: el ángulo de la línea media en el borde de ataque, sobre la línea de \
         corriente. En la libre es el β1B de la línea; en la reglada sale de las rectas de cubo \
         a tapa. ε: envolvimiento. θ borde: posición del borde de ataque en planta; 0 es el \
         punto más adelantado."
            .to_string(),
    );
    if let Some(reference) = wrap_reference(outcome.result.selection.blade_count) {
        small(reference);
    }
    if let Ok(Some(camber)) = camber_of(outcome)
        && let Some(gap) = max_edge_gap_mm(&camber)
    {
        small(format!(
            "Desvío máximo del borde de la reglada: {gap:.1} mm."
        ));
    }
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
             de d1/2, donde empieza el álabe 2D; \"tramo axial\" si la línea es axial en el \
             corte, donde el ángulo en planta no se puede comparar.",
        )
        .small()
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use hm_design::blade_setup::CamberBows;

    #[test]
    fn an_unavailable_shape_cannot_be_chosen() {
        for shape in BladeShape::ALL {
            if shape.is_available() {
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
    fn a_3d_shape_can_be_chosen_and_a_pending_2d_cannot() {
        for shape in [BladeShape::Free3D, BladeShape::Ruled3D] {
            let mut setup = BladeSetup::default();
            assert!(choose_shape(&mut setup, shape), "{shape:?}");
            assert_eq!(setup.shape, shape);
            assert_eq!(arrival_hint(shape), None, "{shape:?}");
        }
        let mut setup = BladeSetup::default();
        assert!(!choose_shape(&mut setup, BladeShape::Circular2D));
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
    fn an_axial_cut_says_so_and_has_no_mismatch() {
        let row = row_of("Tapa".into(), &triangle(Blade2dAngle::Axial));
        assert_eq!(row.beta_2d, "tramo axial");
        assert_eq!(row.mismatch, "—");
        assert!(!row.no_blade && !row.flagged);
    }

    /// La fila de la tapa del diseño por defecto cae en un tramo axial: no
    /// puede salir como un desajuste en color de aviso.
    #[test]
    fn the_shroud_row_of_the_default_design_is_not_flagged() {
        let rows = inlet_rows(&outcome(5)).unwrap();
        let shroud = &rows[rows.len() - 1];
        assert_eq!(shroud.beta_2d, "tramo axial");
        assert!(!shroud.flagged);
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
    fn only_the_ruled_shape_has_the_layer() {
        for (shape, expected) in [
            (BladeShape::Free3D, false),
            (BladeShape::Free2D, false),
            (BladeShape::Ruled3D, true),
        ] {
            let outcome = shaped(shape, 5);
            let model = meridional_model(&outcome.result).unwrap();
            assert_eq!(
                ruled_edge_layer(&outcome, &model).is_some(),
                expected,
                "{shape:?}"
            );
        }
    }

    #[test]
    fn the_ruled_edge_joins_the_two_cuts() {
        let outcome = shaped(BladeShape::Ruled3D, 5);
        let model = meridional_model(&outcome.result).unwrap();
        let (lines, _) = inlet_lines(&outcome).unwrap();
        let layer = ruled_edge_layer(&outcome, &model).unwrap();
        assert_eq!(layer.edge.len(), 5);
        for (got, want) in [
            (layer.edge[0], lines[0].leading_edge),
            (layer.edge[4], lines[4].leading_edge),
        ] {
            // El nodo 0 se interpola linealmente en la polilínea (1e-4 mm de
            // diferencia con el corte); 1 µm sobra y no esconde un desajuste real.
            assert!(
                (got[0] - want[0]).abs() < 1e-3 && (got[1] - want[1]).abs() < 1e-3,
                "{got:?} frente a {want:?}"
            );
        }
    }

    #[test]
    fn the_gap_is_the_largest_of_the_inner_lines() {
        let outcome = shaped(BladeShape::Ruled3D, 5);
        let model = meridional_model(&outcome.result).unwrap();
        let layer = ruled_edge_layer(&outcome, &model).unwrap();
        let camber = camber_of(&outcome).unwrap().unwrap();
        let largest = camber
            .lines
            .iter()
            .map(|line| line.edge_gap.abs() * 1000.0)
            .fold(0.0, f64::max);
        assert!((layer.max_gap_mm - largest).abs() < 1e-9);
        assert!(layer.max_gap_mm > 0.0, "{}", layer.max_gap_mm);
    }

    #[test]
    fn the_pointer_picks_the_nearest_cut_within_reach() {
        let cuts = [[100.0, 100.0], [100.0, 120.0], [300.0, 300.0]];
        assert_eq!(nearest_cut(&cuts, [101.0, 112.0], 12.0), Some(1));
        assert_eq!(nearest_cut(&cuts, [101.0, 104.0], 12.0), Some(0));
        assert_eq!(nearest_cut(&cuts, [200.0, 200.0], 12.0), None);
        assert_eq!(nearest_cut(&[], [0.0, 0.0], 12.0), None);
    }

    /// Un diseño con esa forma 3D (o 2D) y N líneas.
    fn shaped(shape: BladeShape, streamlines: u8) -> Outcome {
        let mut inputs = Inputs::default();
        inputs.blade.shape = shape;
        inputs.blade.streamlines = streamlines;
        compute(&inputs, &Basis::default()).unwrap()
    }

    #[test]
    fn the_3d_table_has_its_thirteen_columns() {
        assert_eq!(
            BLADE_HEADERS,
            [
                "Línea",
                "ψ",
                "r1 [mm]",
                "u1 [m/s]",
                "c1m [m/s]",
                "τ1",
                "β′1 [°]",
                "β1B [°]",
                "β álabe [°]",
                "Δβ [°]",
                "ε [°]",
                "θ borde [°]",
                "L [mm]",
            ]
        );
        let rows = blade_rows(&shaped(BladeShape::Free3D, 5)).unwrap();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].cells.len() + 1, BLADE_HEADERS.len());
    }

    #[test]
    fn in_the_free_shape_every_row_meets_the_flow() {
        let outcome = shaped(BladeShape::Free3D, 5);
        let rows = blade_rows(&outcome).unwrap();
        for row in &rows {
            assert_eq!(row.cells[8], "0.0", "{}", row.label);
            assert!(!row.flagged, "{}", row.label);
            // β álabe y β1B salen al mismo decimal.
            assert_eq!(row.cells[6], row.cells[7], "{}", row.label);
        }
        assert_eq!(rows[0].label, "Cubo");
        assert_eq!(rows[0].cells[9], "181.5");
    }

    #[test]
    fn in_the_ruled_shape_the_inner_rows_show_what_it_costs() {
        let outcome = shaped(BladeShape::Ruled3D, 5);
        let rows = blade_rows(&outcome).unwrap();
        assert_eq!(rows[0].cells[8], "0.0");
        assert_eq!(rows[4].cells[8], "0.0");
        assert!(!rows[0].flagged && !rows[4].flagged);
        for row in &rows[1..4] {
            let delta: f64 = row.cells[8].parse().unwrap();
            assert!(delta < 0.0, "{}: {}", row.label, row.cells[8]);
            assert_eq!(
                row.flagged,
                delta.abs() > INLET_MISMATCH_WARN_DEG,
                "{}",
                row.label
            );
        }
        assert!(rows[1..4].iter().any(|row| row.flagged));
    }

    #[test]
    fn with_the_2d_shape_the_table_is_todays() {
        let outcome = outcome(5);
        assert_eq!(outcome.inputs.blade.shape, BladeShape::Free2D);
        assert!(inlet_rows(&outcome).is_ok());
        assert_eq!(INLET_HEADERS.len(), 10);
        assert_eq!(INLET_HEADERS[8], "β 2D actual [°]");
        assert_eq!(camber_of(&outcome), Ok(None));
    }

    // Review Focus 3: una línea media que falla enseña su mensaje.
    #[test]
    fn a_failing_camber_shows_its_message() {
        let mut outcome = shaped(BladeShape::Free3D, 5);
        outcome.inputs.blade.bows = CamberBows {
            hub: (-30.0, -30.0),
            shroud: (-30.0, -30.0),
        };
        let message = blade_rows(&outcome).unwrap_err();
        assert!(message.contains("línea de corriente"), "{message}");
        // La tabla de entrada de hoy sigue funcionando.
        assert!(inlet_rows(&outcome).is_ok());
    }

    #[test]
    fn the_footer_quotes_gulich_for_five_to_seven_blades() {
        assert_eq!(
            wrap_reference(5).as_deref(),
            Some("Gülich (p. 422): con Z = 5, envolvimiento de 130 a 160°.")
        );
        assert_eq!(
            wrap_reference(7).as_deref(),
            Some("Gülich (p. 422): con Z = 7, envolvimiento de 100 a 130°.")
        );
        assert_eq!(wrap_reference(4), None);
    }

    #[test]
    fn only_the_ruled_shape_reports_the_edge_gap() {
        let ruled = camber_of(&shaped(BladeShape::Ruled3D, 5)).unwrap().unwrap();
        let gap = max_edge_gap_mm(&ruled).expect("la reglada tiene desvío");
        // Oráculo de la Tarea 2: 7,9 mm en la línea 2 de B1.
        assert!((gap - 7.875_984).abs() < 1e-3, "{gap}");
        let free = camber_of(&shaped(BladeShape::Free3D, 5)).unwrap().unwrap();
        assert_eq!(max_edge_gap_mm(&free), None);
    }
}
