//! La fila «Entrada» de la vista «Perfil»: el selector del borde de entrada
//! (romo, elipse o afilado), sus cotas en mm de cubo y de tapa, y las líneas
//! de cp,min (Gülich, ec. 7.1a, p. 414).
//!
//! Las cotas viven en `BladeSetup::edges`, así que elegir un tipo o escribir
//! una cota entra en el historial de «Deshacer» como cualquier otro cambio del
//! álabe. Una cota solo se escribe si con ella el álabe con espesor se sigue
//! construyendo ([`ThicknessGraph::accepts`]); si no, la línea de lectura dice
//! «No se mueve: …» y las entradas quedan como estaban.

use eframe::egui;
use hm_design::blade_camber::BladeCamber;
use hm_design::blade_edges::{BladeEdges, LeadingDims, LeadingKind, cp_min};
use hm_design::blade_setup::{BladeFamily, BladeSetup};
use hm_design::blade_thickness::{ThicknessCurves, ThicknessWall};

use crate::blade_beta::{BetaHistory, BetaSnapshot};
use crate::blade_thickness::ThicknessGraph;
use crate::model::{Inputs, Outcome};

/// Los tipos del selector, en su orden, con su rótulo.
const KINDS: [(LeadingKind, &str); 3] = [
    (LeadingKind::Blunt, "Romo"),
    (LeadingKind::Ellipse, "Elipse"),
    (LeadingKind::Wedge, "Afilado"),
];

/// Una cota de la nariz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    /// Longitud de la nariz, mm.
    Lp,
    /// Espesor de la punta del afilado, mm.
    Tip,
    /// Asimetría del afilado, de 0 a 1.
    Asymmetry,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Self::Lp => "Lp",
            Self::Tip => "punta",
            Self::Asymmetry => "asimetría",
        }
    }

    /// Las cotas que lleva un tipo.
    fn of(kind: LeadingKind) -> &'static [Self] {
        match kind {
            LeadingKind::Blunt => &[],
            LeadingKind::Ellipse => &[Self::Lp],
            LeadingKind::Wedge => &[Self::Lp, Self::Tip, Self::Asymmetry],
        }
    }

    fn value(self, dims: &LeadingDims) -> f64 {
        match self {
            Self::Lp => dims.lp * 1000.0,
            Self::Tip => dims.tip * 1000.0,
            Self::Asymmetry => dims.asymmetry,
        }
    }
}

fn comma(value: f64, decimals: usize) -> String {
    format!("{value:.decimals$}").replace('.', ",")
}

/// El rótulo del selector de un tipo.
pub(super) fn kind_label(kind: LeadingKind) -> &'static str {
    KINDS
        .iter()
        .find(|(candidate, _)| *candidate == kind)
        .map_or("", |(_, label)| label)
}

/// «12,5», «12.5» o «12,5 mm» → 12.5 (mm); la asimetría, un número de 0 a 1.
/// Vacío, texto o un valor fuera de rango es `Err` con la frase que se
/// enseña, y la cota queda como estaba.
pub(super) fn parse_dimension(text: &str, field: Field) -> Result<f64, String> {
    let trimmed = text.trim();
    let example = match field {
        Field::Lp => "12,5",
        Field::Tip => "0,8",
        Field::Asymmetry => "0,5",
    };
    if trimmed.is_empty() {
        return Err(format!(
            "escribe {} {}, por ejemplo {example}",
            if field == Field::Asymmetry {
                "la"
            } else {
                "una cota"
            },
            if field == Field::Asymmetry {
                "asimetría (de 0 a 1)"
            } else {
                "en mm"
            }
        ));
    }
    let lower = trimmed.to_lowercase();
    let number = lower.strip_suffix("mm").unwrap_or(&lower).trim();
    let value: f64 = number.replace(',', ".").parse().map_err(|_| {
        format!(
            "«{trimmed}» no es {}; escribe por ejemplo {example}",
            if field == Field::Asymmetry {
                "una asimetría"
            } else {
                "una cota en mm"
            }
        )
    })?;
    if !value.is_finite() {
        return Err(format!("«{trimmed}» no es un número finito"));
    }
    match field {
        Field::Lp if value <= 0.0 => Err("Lp tiene que ser mayor que cero".to_string()),
        Field::Tip if value < 0.0 => Err("la punta no puede ser negativa".to_string()),
        Field::Asymmetry if !(0.0..=1.0).contains(&value) => {
            Err("la asimetría va de 0 a 1".to_string())
        }
        _ => Ok(value),
    }
}

/// El álabe con esa cota de `wall` escrita (con «cubo = tapa», en las dos
/// paredes). Un tipo sin esa cota, o un texto que no vale, es `Err` y no
/// cambia nada. Los topes que dependen de e y de L los acota el sólido y los
/// avisa.
pub(super) fn write_dimension(
    setup: &BladeSetup,
    wall: ThicknessWall,
    field: Field,
    text: &str,
) -> Result<BladeSetup, String> {
    let kind = setup.edges.leading;
    if !Field::of(kind).contains(&field) {
        return Err(format!(
            "el borde «{}» no lleva la cota {}",
            kind_label(kind).to_lowercase(),
            field.label()
        ));
    }
    let value = parse_dimension(text, field)?;
    let same = setup.thickness.same;
    let current = if same || wall == ThicknessWall::Hub {
        setup.edges.leading_hub
    } else {
        setup.edges.leading_shroud
    };
    let dims = match field {
        Field::Lp => LeadingDims {
            lp: value / 1000.0,
            ..current
        },
        Field::Tip => LeadingDims {
            tip: value / 1000.0,
            ..current
        },
        Field::Asymmetry => LeadingDims {
            asymmetry: value,
            ..current
        },
    };
    let mut out = setup.clone();
    out.edges = setup.edges.with_leading_dims(wall, dims, same);
    Ok(out)
}

/// El álabe con ese tipo de entrada y las cotas por defecto de cada pared
/// (con su e(0)).
pub(super) fn with_kind(
    setup: &BladeSetup,
    kind: LeadingKind,
    e_hub: f64,
    e_shroud: f64,
) -> BladeSetup {
    let mut out = setup.clone();
    out.edges = setup.edges.with_leading_kind(kind, e_hub, e_shroud);
    out
}

/// Enciende o apaga «cubo = tapa» para la curva de espesor y para los bordes:
/// un solo interruptor gobierna las dos cosas.
pub(super) fn switch_same(setup: &BladeSetup, base: &ThicknessCurves, same: bool) -> BladeSetup {
    let mut out = setup.clone();
    out.thickness = setup.thickness.with_same(base, same);
    out.edges = setup.edges.with_same(same);
    out
}

/// Las líneas de cp,min de la entrada, por pared (una sola con «cubo =
/// tapa»), con e(0) de cada pared y la Lp escrita: «Cubo: e/Lp = 0,20 →
/// cp,min = 0,155». Con romo, la nota de que es peor que el semicírculo. Sin
/// umbral: es una lectura orientativa (Gülich avisa de que no se transfiere
/// tal cual al rodete).
#[must_use]
pub(super) fn cp_min_lines(
    edges: &BladeEdges,
    same: bool,
    e_hub: f64,
    e_shroud: f64,
) -> Vec<String> {
    if edges.leading == LeadingKind::Blunt {
        return vec!["Entrada: romo, peor que el semicírculo (cp,min > 2,05)".to_string()];
    }
    let walls: Vec<(&str, f64, f64)> = if same {
        vec![("Cubo = tapa", 0.0, e_hub)]
    } else {
        vec![("Cubo", 0.0, e_hub), ("Tapa", 1.0, e_shroud)]
    };
    walls
        .into_iter()
        .map(|(name, psi, e)| {
            let profile = edges.profile_at(psi, same);
            let Some(ratio) = profile.ratio(e) else {
                return format!("{name}: sin Lp");
            };
            let mut line = format!(
                "{name}: e/Lp = {} → cp,min = {}",
                comma(ratio, 2),
                comma(cp_min(ratio), 3)
            );
            if ratio >= 2.0 {
                line.push_str(" · más romo que un semicírculo");
            }
            if edges.leading == LeadingKind::Wedge && profile.dims.asymmetry > 0.0 {
                line.push_str(" · sin corrección por asimetría");
            }
            line
        })
        .collect()
}

/// Lo que se está escribiendo: la cota y su texto.
#[derive(Debug, Clone)]
struct Editing {
    wall: ThicknessWall,
    field: Field,
    text: String,
    /// Recién abierta: pide el foco una vez.
    opened: bool,
}

fn editing_id() -> egui::Id {
    egui::Id::unique("leading_edit")
}

fn status_id() -> egui::Id {
    egui::Id::unique("leading_status")
}

/// Escribe `candidate` en las entradas si el álabe con espesor se sigue
/// construyendo con él. `Ok(true)` si cambió algo; `Err(motivo)` y nada
/// escrito si no se construye.
pub(super) fn commit(
    outcome: &Outcome,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
    graph: &ThicknessGraph,
    candidate: BladeSetup,
    time: f64,
) -> Result<bool, String> {
    if candidate == inputs.blade {
        return Ok(false);
    }
    graph.accepts(outcome, &candidate)?;
    inputs.blade = candidate;
    history.record(&BetaSnapshot::of(inputs), time);
    Ok(true)
}

/// «Entrada: [Romo | Elipse | Afilado]» y, debajo, las cotas del tipo en mm de
/// cubo y de tapa (una sola fila con «cubo = tapa»). Con una forma 2D sale
/// desactivada y no toca las cotas guardadas. `true` si escribió `inputs`.
pub(super) fn leading_row(
    ui: &mut egui::Ui,
    outcome: &Outcome,
    inputs: &mut Inputs,
    history: &mut BetaHistory,
    camber: Option<&BladeCamber>,
) -> bool {
    let kind = inputs.blade.edges.leading;
    let camber = camber.filter(|_| inputs.blade.shape.family() == BladeFamily::ThreeD);
    let Some(camber) = camber else {
        ui.add_enabled_ui(false, |ui| {
            ui.horizontal(|ui| {
                ui.label("Entrada:");
                for (candidate, label) in KINDS {
                    let _ = ui.selectable_label(candidate == kind, label);
                }
            });
        });
        return false;
    };
    let graph = match ThicknessGraph::of(outcome, camber) {
        Ok(graph) => graph,
        Err(message) => {
            ui.label(format!("Sin curva de espesor: {message}."));
            return false;
        }
    };
    let (e_hub, e_shroud) = (graph.curves.hub.at(0.0), graph.curves.shroud.at(0.0));
    let ctx = ui.ctx().clone();
    let time = ui.input(|input| input.time);
    let mut edited = false;
    let mut status: Option<String> = None;

    ui.horizontal(|ui| {
        ui.label("Entrada:");
        let mut chosen = kind;
        for (candidate, label) in KINDS {
            ui.selectable_value(&mut chosen, candidate, label);
        }
        if chosen != kind {
            let candidate = with_kind(&inputs.blade, chosen, e_hub, e_shroud);
            match commit(outcome, inputs, history, &graph, candidate, time) {
                Ok(changed) => edited |= changed,
                Err(reason) => status = Some(format!("No se mueve: {reason}")),
            }
            ctx.data_mut(|data| data.remove::<Editing>(editing_id()));
        }
    });

    let same = inputs.blade.thickness.same;
    let kind = inputs.blade.edges.leading;
    let walls: &[(ThicknessWall, &str)] = if same {
        &[(ThicknessWall::Hub, "Cubo = tapa")]
    } else {
        &[
            (ThicknessWall::Hub, "Cubo"),
            (ThicknessWall::Shroud, "Tapa"),
        ]
    };
    for &(wall, name) in walls {
        if Field::of(kind).is_empty() {
            break;
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(name).small().weak());
            for &field in Field::of(kind) {
                let dims = if wall == ThicknessWall::Hub || same {
                    inputs.blade.edges.leading_hub
                } else {
                    inputs.blade.edges.leading_shroud
                };
                let shown = match field {
                    Field::Asymmetry => {
                        format!("{} {}", field.label(), comma(field.value(&dims), 2))
                    }
                    _ => format!("{} {} mm", field.label(), comma(field.value(&dims), 2)),
                };
                let editing = ctx.data(|data| data.get_temp::<Editing>(editing_id()));
                match editing.filter(|e| e.wall == wall && e.field == field) {
                    Some(mut editing) => {
                        let response = ui
                            .add(egui::TextEdit::singleline(&mut editing.text).desired_width(64.0));
                        if editing.opened {
                            response.request_focus();
                            editing.opened = false;
                        }
                        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                        if escape {
                            ctx.data_mut(|data| data.remove::<Editing>(editing_id()));
                        } else if response.lost_focus() {
                            ctx.data_mut(|data| data.remove::<Editing>(editing_id()));
                            match write_dimension(&inputs.blade, wall, field, &editing.text) {
                                Ok(candidate) => {
                                    match commit(outcome, inputs, history, &graph, candidate, time)
                                    {
                                        Ok(changed) => edited |= changed,
                                        Err(reason) => {
                                            status = Some(format!("No se mueve: {reason}"));
                                        }
                                    }
                                }
                                Err(reason) => status = Some(format!("Sin cambio: {reason}")),
                            }
                        } else {
                            ctx.data_mut(|data| data.insert_temp(editing_id(), editing));
                        }
                    }
                    None => {
                        let response = ui
                            .add(egui::Label::new(shown).sense(egui::Sense::click()))
                            .on_hover_text(
                                "Doble clic para escribir la cota (coma o punto decimal).",
                            );
                        if response.double_clicked() {
                            let text = match field {
                                Field::Asymmetry => comma(field.value(&dims), 2),
                                _ => comma(field.value(&dims), 2),
                            };
                            ctx.data_mut(|data| {
                                data.insert_temp(
                                    editing_id(),
                                    Editing {
                                        wall,
                                        field,
                                        text,
                                        opened: true,
                                    },
                                );
                            });
                        }
                    }
                }
                ui.add_space(6.0);
            }
        });
    }
    if edited {
        ctx.data_mut(|data| data.remove::<String>(status_id()));
    }
    if let Some(text) = status {
        ctx.data_mut(|data| data.insert_temp(status_id(), text));
    }
    if let Some(text) = ctx.data(|data| data.get_temp::<String>(status_id())) {
        ui.label(
            egui::RichText::new(text)
                .small()
                .color(crate::theme::palette(&ctx).error),
        );
    }
    for line in cp_min_lines(&inputs.blade.edges, same, e_hub, e_shroud) {
        ui.label(egui::RichText::new(line).small().weak());
    }
    edited
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blade_inlet::camber_of;
    use crate::model::{Basis, compute};

    fn b1_3d() -> (Outcome, BladeCamber) {
        let mut inputs = Inputs::default();
        inputs.blade.shape = hm_design::blade_setup::BladeShape::Free3D;
        let outcome = compute(&inputs, &Basis::default()).unwrap();
        let camber = camber_of(&outcome).unwrap().expect("forma 3D");
        (outcome, camber)
    }

    fn graph(outcome: &Outcome, camber: &BladeCamber) -> ThicknessGraph {
        ThicknessGraph::of(outcome, camber).unwrap()
    }

    fn e_walls(graph: &ThicknessGraph) -> (f64, f64) {
        (graph.curves.hub.at(0.0), graph.curves.shroud.at(0.0))
    }

    fn history_for(inputs: &Inputs) -> BetaHistory {
        let mut history = BetaHistory::default();
        history.follow(&BetaSnapshot::of(inputs));
        history
    }

    /// Elige un tipo como lo hace la fila: por `commit`.
    fn choose(
        outcome: &Outcome,
        inputs: &mut Inputs,
        history: &mut BetaHistory,
        graph: &ThicknessGraph,
        kind: LeadingKind,
        time: f64,
    ) -> Result<bool, String> {
        let (e_hub, e_shroud) = e_walls(graph);
        let candidate = with_kind(&inputs.blade, kind, e_hub, e_shroud);
        commit(outcome, inputs, history, graph, candidate, time)
    }

    #[test]
    fn the_default_row_says_blunt() {
        let (outcome, _) = b1_3d();
        assert_eq!(outcome.inputs.blade.edges.leading, LeadingKind::Blunt);
        assert_eq!(kind_label(LeadingKind::Blunt), "Romo");
        let lines = cp_min_lines(&outcome.inputs.blade.edges, true, 0.006, 0.006);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("romo"), "{lines:?}");
        assert!(lines[0].contains("2,05"), "{lines:?}");
    }

    #[test]
    fn choosing_ellipse_sets_the_default_dims_on_both_walls() {
        let (outcome, camber) = b1_3d();
        let graph = graph(&outcome, &camber);
        let mut inputs = outcome.inputs.clone();
        let mut history = history_for(&inputs);
        assert_eq!(
            choose(
                &outcome,
                &mut inputs,
                &mut history,
                &graph,
                LeadingKind::Ellipse,
                1.0
            ),
            Ok(true)
        );
        let (e_hub, e_shroud) = e_walls(&graph);
        let edges = &inputs.blade.edges;
        assert_eq!(edges.leading, LeadingKind::Ellipse);
        assert_eq!(edges.leading_hub.lp, 5.0 * e_hub);
        assert_eq!(edges.leading_shroud.lp, 5.0 * e_shroud);
        // e/Lp = 0,20 en cubo y en tapa.
        for (e, dims) in [(e_hub, edges.leading_hub), (e_shroud, edges.leading_shroud)] {
            assert!((e / dims.lp - 0.2).abs() < 1e-12);
        }
        assert!(history.can_undo());
        // Elegir el mismo tipo no escribe ni anota nada.
        let mut again = history_for(&inputs);
        assert_eq!(
            choose(
                &outcome,
                &mut inputs.clone(),
                &mut again,
                &graph,
                LeadingKind::Ellipse,
                2.0
            )
            .map(|_| ()),
            Ok(())
        );
    }

    #[test]
    fn typing_a_dimension_with_a_comma_is_accepted() {
        let (outcome, camber) = b1_3d();
        let graph = graph(&outcome, &camber);
        let mut inputs = outcome.inputs.clone();
        let mut history = history_for(&inputs);
        choose(
            &outcome,
            &mut inputs,
            &mut history,
            &graph,
            LeadingKind::Ellipse,
            1.0,
        )
        .unwrap();
        for (text, expected) in [("12,5", 0.0125), ("12.5", 0.0125), ("12,5 mm", 0.0125)] {
            let candidate =
                write_dimension(&inputs.blade, ThicknessWall::Hub, Field::Lp, text).unwrap();
            assert_eq!(candidate.edges.leading_hub.lp, expected, "{text}");
            // Con «cubo = tapa» (el de por defecto) la tapa copia el cubo.
            assert_eq!(candidate.edges.leading_shroud.lp, expected, "{text}");
        }
        let before = inputs.blade.clone();
        for text in ["", "abc", "0", "-3", "inf", "1e999"] {
            let error =
                write_dimension(&before, ThicknessWall::Hub, Field::Lp, text).expect_err(text);
            assert!(!error.is_empty());
        }
        // Y el borde romo no lleva cotas.
        let blunt = BladeSetup::default();
        assert!(write_dimension(&blunt, ThicknessWall::Hub, Field::Lp, "10").is_err());
        // La elipse no lleva punta ni asimetría.
        assert!(write_dimension(&before, ThicknessWall::Hub, Field::Tip, "1").is_err());
        // La asimetría va de 0 a 1.
        let mut wedge = before.clone();
        wedge.edges = wedge
            .edges
            .with_leading_kind(LeadingKind::Wedge, 0.006, 0.006);
        assert!(write_dimension(&wedge, ThicknessWall::Hub, Field::Asymmetry, "1,2").is_err());
        let ok = write_dimension(&wedge, ThicknessWall::Hub, Field::Asymmetry, "0,5").unwrap();
        assert_eq!(ok.edges.leading_hub.asymmetry, 0.5);
        // Con coma, la punta en mm.
        let ok = write_dimension(&wedge, ThicknessWall::Hub, Field::Tip, "0,9").unwrap();
        assert_eq!(ok.edges.leading_hub.tip, 0.0009);
        // El historial: lo escrito se anota y se commitea.
        let candidate =
            write_dimension(&inputs.blade, ThicknessWall::Hub, Field::Lp, "12,5").unwrap();
        assert_eq!(
            commit(&outcome, &mut inputs, &mut history, &graph, candidate, 5.0),
            Ok(true)
        );
        assert_eq!(inputs.blade.edges.leading_hub.lp, 0.0125);
    }

    #[test]
    fn a_dimension_that_breaks_the_blade_is_refused() {
        // Con 60 mm en el nodo central los álabes vecinos se tocan: el sólido
        // no se construye con ninguna nariz, y la cota no se escribe.
        let (outcome, camber) = b1_3d();
        let graph = graph(&outcome, &camber);
        let mut inputs = outcome.inputs.clone();
        inputs.blade.thickness =
            graph
                .setup
                .with_node_moved(&graph.curves, ThicknessWall::Hub, 1, 0.5, 0.060);
        let mut history = history_for(&inputs);
        let (e_hub, e_shroud) = e_walls(&graph);
        inputs.blade.edges =
            BladeEdges::default().with_leading_kind(LeadingKind::Ellipse, e_hub, e_shroud);
        let before = inputs.blade.clone();
        let candidate =
            write_dimension(&inputs.blade, ThicknessWall::Hub, Field::Lp, "40").unwrap();
        let reason = commit(&outcome, &mut inputs, &mut history, &graph, candidate, 3.0)
            .expect_err("con 60 mm el álabe no se construye");
        assert!(reason.contains("(0 es el cubo)"), "{reason}");
        // No se escribe nada y no se anota nada.
        assert_eq!(inputs.blade, before);
        assert!(!history.can_undo());
    }

    #[test]
    fn changing_the_kind_keeps_the_dims_of_the_other_one_in_the_history() {
        let (outcome, camber) = b1_3d();
        let graph = graph(&outcome, &camber);
        let mut inputs = outcome.inputs.clone();
        let mut history = history_for(&inputs);
        choose(
            &outcome,
            &mut inputs,
            &mut history,
            &graph,
            LeadingKind::Ellipse,
            1.0,
        )
        .unwrap();
        let candidate =
            write_dimension(&inputs.blade, ThicknessWall::Hub, Field::Lp, "25").unwrap();
        commit(&outcome, &mut inputs, &mut history, &graph, candidate, 3.0).unwrap();
        let ellipse = inputs.blade.clone();
        assert_eq!(ellipse.edges.leading_hub.lp, 0.025);

        // Elipse → afilado: las cotas se rehacen con las del afilado…
        choose(
            &outcome,
            &mut inputs,
            &mut history,
            &graph,
            LeadingKind::Wedge,
            6.0,
        )
        .unwrap();
        assert_eq!(inputs.blade.edges.leading, LeadingKind::Wedge);
        assert_ne!(inputs.blade.edges.leading_hub.lp, 0.025);
        // …y «Deshacer» devuelve la elipse con sus cotas.
        history.follow(&BetaSnapshot::of(&inputs));
        let previous = history.undo(&BetaSnapshot::of(&inputs)).unwrap();
        previous.apply(&mut inputs);
        assert_eq!(inputs.blade, ellipse);
        // Y «Rehacer», el afilado.
        let next = history.redo(&BetaSnapshot::of(&inputs)).unwrap();
        next.apply(&mut inputs);
        assert_eq!(inputs.blade.edges.leading, LeadingKind::Wedge);
    }

    #[test]
    fn cp_min_lines_match_the_book_values() {
        let e = 0.006;
        let edges = BladeEdges::default().with_leading_kind(LeadingKind::Ellipse, e, e);
        let lines = cp_min_lines(&edges, true, e, e);
        assert_eq!(lines, ["Cubo = tapa: e/Lp = 0,20 → cp,min = 0,155"]);
        let split = cp_min_lines(&edges, false, e, e);
        assert_eq!(split.len(), 2);
        assert!(split[0].starts_with("Cubo:") && split[1].starts_with("Tapa:"));
        assert!(split[1].contains("0,155"), "{split:?}");
        // El semicírculo (e/Lp = 2): 2,05, y el aviso de que es más romo.
        let round = BladeEdges {
            leading: LeadingKind::Ellipse,
            leading_hub: LeadingDims {
                lp: e / 2.0,
                tip: 0.0,
                asymmetry: 0.0,
            },
            leading_shroud: LeadingDims::default(),
        };
        let lines = cp_min_lines(&round, true, e, e);
        assert!(lines[0].contains("2,0"), "{lines:?}");
        assert!(
            lines[0].contains("más romo que un semicírculo"),
            "{lines:?}"
        );
        // El afilado asimétrico lleva la nota.
        let mut wedge = BladeEdges::default().with_leading_kind(LeadingKind::Wedge, e, e);
        assert!(!cp_min_lines(&wedge, true, e, e)[0].contains("asimetría"));
        wedge.leading_hub.asymmetry = 0.5;
        assert!(cp_min_lines(&wedge, true, e, e)[0].contains("sin corrección por asimetría"));
    }

    #[test]
    fn the_switch_joins_and_splits_the_dims() {
        let (outcome, camber) = b1_3d();
        let graph = graph(&outcome, &camber);
        let (e_hub, e_shroud) = e_walls(&graph);
        let mut setup = with_kind(&outcome.inputs.blade, LeadingKind::Ellipse, e_hub, e_shroud);
        // Apagar «cubo = tapa»: la tapa arranca como copia del cubo.
        setup = switch_same(&setup, &graph.curves, false);
        assert!(!setup.thickness.same);
        assert_eq!(setup.edges.leading_shroud, setup.edges.leading_hub);
        // Con las paredes separadas, escribir en la tapa no toca el cubo.
        let tapa = write_dimension(&setup, ThicknessWall::Shroud, Field::Lp, "20").unwrap();
        assert_eq!(tapa.edges.leading_shroud.lp, 0.020);
        assert_eq!(tapa.edges.leading_hub, setup.edges.leading_hub);
        // Encender: manda el cubo.
        let joined = switch_same(&tapa, &graph.curves, true);
        assert!(joined.thickness.same);
        assert_eq!(joined.edges.leading_shroud, tapa.edges.leading_hub);
        // Con «cubo = tapa», escribir en cualquier pared escribe las dos.
        let both = write_dimension(&joined, ThicknessWall::Shroud, Field::Lp, "22").unwrap();
        assert_eq!(both.edges.leading_hub.lp, 0.022);
        assert_eq!(both.edges.leading_shroud.lp, 0.022);
    }

    #[test]
    fn a_2d_shape_disables_the_row_and_keeps_the_stored_edges() {
        let mut inputs = Inputs::default();
        inputs.blade.shape = hm_design::blade_setup::BladeShape::Free2D;
        inputs.blade.edges =
            BladeEdges::default().with_leading_kind(LeadingKind::Wedge, 0.006, 0.006);
        let outcome = compute(&inputs, &Basis::default()).unwrap();
        let before = inputs.clone();
        let mut history = BetaHistory::default();
        let mut edited = true;
        egui::__run_test_ui(|ui| {
            edited = leading_row(ui, &outcome, &mut inputs, &mut history, None);
        });
        assert!(!edited);
        assert_eq!(inputs, before);
        assert_eq!(inputs.blade.edges.leading, LeadingKind::Wedge);
    }

    #[test]
    fn the_row_draws_without_panicking() {
        let (outcome, camber) = b1_3d();
        for kind in [LeadingKind::Blunt, LeadingKind::Ellipse, LeadingKind::Wedge] {
            let graph = graph(&outcome, &camber);
            let (e_hub, e_shroud) = e_walls(&graph);
            for same in [true, false] {
                let mut inputs = outcome.inputs.clone();
                inputs.blade = with_kind(&inputs.blade, kind, e_hub, e_shroud);
                inputs.blade = switch_same(&inputs.blade, &graph.curves, same);
                let mut history = history_for(&inputs);
                egui::__run_test_ui(|ui| {
                    leading_row(ui, &outcome, &mut inputs, &mut history, Some(&camber));
                });
            }
        }
    }
}
