//! Prototipo del paso A: cortar una tapa sobre la curva y convertir un tramo
//! en recta. Es solo geometría mecánica encima de la Bézier hidráulica, que
//! no cambia: los cortes son parámetros `t` sobre ella y cada tramo es curva
//! (la Bézier entre sus dos cortes) o recta (la cuerda entre ellos).
//!
//! De momento vive en la memoria de la ventana: no se guarda en el `.hmf` ni
//! entra en deshacer. Sirve para que Jorge pruebe la interacción.

use eframe::egui;
use egui::Color32;
use egui_plot::{PlotPoint, Points};
use hm_design::meridian::{MeridionalModel, Wall};

use crate::curves::line;

/// Distancia en píxeles para tocar una tapa con el ratón.
const TOUCH_PIXELS: f32 = 12.0;

/// Hueco mínimo, en `t`, entre dos cortes o entre un corte y un extremo.
const MIN_GAP: f64 = 0.005;

/// Puntos con que se dibuja cada tramo curvo.
const SEGMENT_POINTS: usize = 80;

/// Color del tramo seleccionado.
const SELECTED: Color32 = Color32::from_rgb(230, 120, 0);

/// Cortes de una tapa y el tipo de cada tramo. `straight` tiene un elemento
/// más que `cuts`: el tramo `i` va del corte `i - 1` (o `t = 0`) al `i` (o `t = 1`).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct WallSegments {
    cuts: Vec<f64>,
    straight: Vec<bool>,
}

impl Default for WallSegments {
    fn default() -> Self {
        Self {
            cuts: Vec::new(),
            straight: vec![false],
        }
    }
}

impl WallSegments {
    fn is_plain(&self) -> bool {
        self.cuts.is_empty()
    }

    fn has_straight(&self) -> bool {
        self.straight.iter().any(|s| *s)
    }

    /// Los `t` de inicio y final del tramo `index`.
    fn bounds(&self, index: usize) -> (f64, f64) {
        let start = if index == 0 {
            0.0
        } else {
            self.cuts[index - 1]
        };
        let end = self.cuts.get(index).copied().unwrap_or(1.0);
        (start, end)
    }

    /// El tramo que contiene `t`.
    fn segment_of(&self, t: f64) -> usize {
        self.cuts.iter().take_while(|cut| **cut < t).count()
    }

    /// Por qué no se puede cortar en `t`, o `None` si se puede.
    fn cut_refusal(&self, t: f64) -> Option<&'static str> {
        let index = self.segment_of(t);
        let (start, end) = self.bounds(index);
        if t - start < MIN_GAP || end - t < MIN_GAP {
            Some("demasiado cerca de un corte o de un extremo")
        } else if self.straight[index] {
            Some("el tramo es recta: vuélvelo a curva para cortarlo")
        } else {
            None
        }
    }

    /// Corta en `t`: el tramo curvo se parte en dos curvos que dibujan lo mismo.
    fn cut(&mut self, t: f64) -> bool {
        if self.cut_refusal(t).is_some() {
            return false;
        }
        let index = self.segment_of(t);
        self.cuts.insert(index, t);
        self.straight.insert(index, false);
        true
    }

    /// Quita el corte `index` y une sus dos tramos: recta si los dos lo eran.
    fn remove_cut(&mut self, index: usize) {
        if index >= self.cuts.len() {
            return;
        }
        self.cuts.remove(index);
        let merged = self.straight[index] && self.straight[index + 1];
        self.straight.remove(index + 1);
        self.straight[index] = merged;
    }

    /// Cada tramo como polilínea sobre la Bézier `points`.
    fn contour(&self, points: &[[f64; 2]; 5]) -> Vec<Vec<[f64; 2]>> {
        (0..self.straight.len())
            .map(|index| {
                let (start, end) = self.bounds(index);
                if self.straight[index] {
                    vec![point_at(points, start), point_at(points, end)]
                } else {
                    (0..=SEGMENT_POINTS)
                        .map(|k| {
                            let t = start + (end - start) * k as f64 / SEGMENT_POINTS as f64;
                            point_at(points, t)
                        })
                        .collect()
                }
            })
            .collect()
    }
}

/// Los cortes de las dos tapas.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Segments {
    walls: [WallSegments; 2],
}

impl Segments {
    fn wall(&self, wall: Wall) -> &WallSegments {
        &self.walls[wall_index(wall)]
    }

    fn wall_mut(&mut self, wall: Wall) -> &mut WallSegments {
        &mut self.walls[wall_index(wall)]
    }
}

fn wall_index(wall: Wall) -> usize {
    match wall {
        Wall::Shroud => 0,
        Wall::Hub => 1,
    }
}

/// Punto de la Bézier de grado 4 en `t`, por de Casteljau.
pub(super) fn point_at(points: &[[f64; 2]; 5], t: f64) -> [f64; 2] {
    let mut p = *points;
    for level in (1..5).rev() {
        for i in 0..level {
            p[i] = [
                p[i][0] + (p[i + 1][0] - p[i][0]) * t,
                p[i][1] + (p[i + 1][1] - p[i][1]) * t,
            ];
        }
    }
    p[0]
}

/// El `t` del punto de la Bézier más cercano a `at` y su distancia, en las
/// unidades de `distance`: un barrido grueso y después un refinado local.
fn nearest_t(
    points: &[[f64; 2]; 5],
    at: [f64; 2],
    distance: impl Fn([f64; 2], [f64; 2]) -> f64,
) -> (f64, f64) {
    const SWEEP: usize = 400;
    let d = |t: f64| distance(point_at(points, t), at);
    let mut best = (0..=SWEEP)
        .map(|k| k as f64 / SWEEP as f64)
        .map(|t| (t, d(t)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0.0, f64::INFINITY));
    let mut step = 1.0 / SWEEP as f64;
    for _ in 0..30 {
        step *= 0.5;
        for t in [best.0 - step, best.0 + step] {
            let t = t.clamp(0.0, 1.0);
            let value = d(t);
            if value < best.1 {
                best = (t, value);
            }
        }
    }
    best
}

/// Lo que se tocó con el clic derecho.
#[derive(Clone, Copy, Debug)]
struct Target {
    wall: Wall,
    t: f64,
    /// Un corte a pocos píxeles del clic.
    near_cut: Option<usize>,
}

fn state_id() -> egui::Id {
    egui::Id::unique("meridian_segments_state")
}

fn target_id() -> egui::Id {
    egui::Id::unique("meridian_segments_target")
}

fn selected_id() -> egui::Id {
    egui::Id::unique("meridian_segments_selected")
}

fn load(ctx: &egui::Context) -> Segments {
    seed_from_env(ctx);
    ctx.data(|data| data.get_temp::<Segments>(state_id()))
        .unwrap_or_default()
}

fn store(ctx: &egui::Context, segments: Segments) {
    ctx.data_mut(|data| data.insert_temp(state_id(), segments));
}

/// Si la tapa tiene algún tramo en recta: entonces la Bézier hidráulica se
/// dibuja de referencia, en gris, y el contorno mecánico encima.
pub(super) fn reference_only(ctx: &egui::Context, wall: Wall) -> bool {
    load(ctx).wall(wall).has_straight()
}

/// La tapa y el `t` más cercanos a `pointer`, si quedan a pocos píxeles.
fn touched(
    plot_ui: &egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    pointer: PlotPoint,
) -> Option<(Wall, f64)> {
    let screen = plot_ui.screen_from_plot(pointer);
    let distance = |p: [f64; 2], _: [f64; 2]| {
        let s = plot_ui.screen_from_plot(PlotPoint::new(p[0], p[1]));
        f64::from(s.distance(screen))
    };
    Wall::ALL
        .into_iter()
        .map(|wall| {
            let (t, d) = nearest_t(
                &model.params.wall(wall).points,
                [pointer.x, pointer.y],
                distance,
            );
            (wall, t, d)
        })
        .filter(|(_, _, d)| *d <= f64::from(TOUCH_PIXELS))
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(wall, t, _)| (wall, t))
}

/// El corte de `wall` a pocos píxeles de `pointer`.
fn near_cut(
    plot_ui: &egui_plot::PlotUi<'_>,
    model: &MeridionalModel,
    segments: &WallSegments,
    wall: Wall,
    pointer: PlotPoint,
) -> Option<usize> {
    let screen = plot_ui.screen_from_plot(pointer);
    let points = &model.params.wall(wall).points;
    segments
        .cuts
        .iter()
        .enumerate()
        .map(|(index, t)| {
            let p = point_at(points, *t);
            let s = plot_ui.screen_from_plot(PlotPoint::new(p[0], p[1]));
            (index, s.distance(screen))
        })
        .filter(|(_, d)| *d <= TOUCH_PIXELS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// Dibuja el contorno mecánico de las tapas cortadas, el tramo seleccionado
/// y los puntos de corte.
pub(super) fn draw(plot_ui: &mut egui_plot::PlotUi<'_>, model: &MeridionalModel) {
    let segments = load(plot_ui.ctx());
    let selected: Option<(Wall, usize)> = plot_ui.ctx().data(|data| data.get_temp(selected_id()));
    for wall in Wall::ALL {
        let state = segments.wall(wall);
        if state.is_plain() && !state.has_straight() {
            continue;
        }
        let points = &model.params.wall(wall).points;
        let colour = crate::meridian::meridian_color(wall.name());
        for (index, piece) in state.contour(points).into_iter().enumerate() {
            let chosen = selected == Some((wall, index));
            plot_ui.line(
                line(
                    "Contorno mecánico",
                    piece,
                    if chosen { SELECTED } else { colour },
                )
                .width(if chosen { 4.0 } else { 2.5 }),
            );
        }
        let cuts: Vec<[f64; 2]> = state.cuts.iter().map(|t| point_at(points, *t)).collect();
        plot_ui.points(
            Points::new("Cortes", cuts.clone())
                .shape(egui_plot::MarkerShape::Circle)
                .radius(5.0)
                .color(Color32::WHITE),
        );
        plot_ui.points(
            Points::new("Cortes", cuts)
                .shape(egui_plot::MarkerShape::Circle)
                .radius(5.0)
                .filled(false)
                .color(Color32::from_gray(20)),
        );
    }
}

/// Clic sobre una tapa cortada: selecciona el tramo. Clic derecho: el menú
/// con «Cortar aquí», «Convertir en recta» y lo demás.
pub(super) fn interact(plot_ui: &mut egui_plot::PlotUi<'_>, model: &MeridionalModel) {
    let ctx = plot_ui.ctx().clone();
    let response = plot_ui.response().clone();
    let mut segments = load(&ctx);
    if let Some(pointer) = plot_ui.pointer_coordinate() {
        if response.secondary_clicked() {
            let target = touched(plot_ui, model, pointer).map(|(wall, t)| Target {
                wall,
                t,
                near_cut: near_cut(plot_ui, model, segments.wall(wall), wall, pointer),
            });
            if let Some(target) = target {
                let index = segments.wall(target.wall).segment_of(target.t);
                ctx.data_mut(|data| data.insert_temp(selected_id(), (target.wall, index)));
            }
            ctx.data_mut(|data| data.insert_temp(target_id(), target));
        }
        if response.clicked() {
            match touched(plot_ui, model, pointer) {
                Some((wall, t)) if !segments.wall(wall).is_plain() => {
                    let index = segments.wall(wall).segment_of(t);
                    ctx.data_mut(|data| data.insert_temp(selected_id(), (wall, index)));
                }
                _ => ctx.data_mut(|data| data.remove::<(Wall, usize)>(selected_id())),
            }
        }
    }
    let target: Option<Target> = ctx.data(|data| data.get_temp(target_id())).flatten();
    let mut changed = false;
    response.context_menu(|ui| {
        let Some(target) = target else {
            ui.label("Haz clic derecho sobre una tapa para cortarla.");
            return;
        };
        let points = model.params.wall(target.wall).points;
        let state = segments.wall_mut(target.wall);
        let at = point_at(&points, target.t);
        ui.label(
            egui::RichText::new(format!(
                "{} · z = {:.2} mm, r = {:.2} mm",
                target.wall.name(),
                at[0],
                at[1]
            ))
            .small()
            .weak(),
        );
        ui.separator();
        match state.cut_refusal(target.t) {
            None => {
                if ui.button("Cortar aquí").clicked() {
                    changed |= state.cut(target.t);
                    ui.close();
                }
            }
            Some(why) => {
                ui.add_enabled(false, egui::Button::new("Cortar aquí"))
                    .on_disabled_hover_text(why);
            }
        }
        let index = state.segment_of(target.t);
        if !state.is_plain() {
            let label = if state.straight[index] {
                "Volver a curva"
            } else {
                "Convertir en recta"
            };
            if ui.button(label).clicked() {
                state.straight[index] = !state.straight[index];
                changed = true;
                ui.close();
            }
        }
        if let Some(cut) = target.near_cut
            && ui.button("Quitar este corte").clicked()
        {
            state.remove_cut(cut);
            changed = true;
            ui.close();
        }
        if !state.is_plain() && ui.button("Quitar todos los cortes de esta tapa").clicked() {
            *state = WallSegments::default();
            changed = true;
            ui.close();
        }
    });
    if changed {
        // Los índices de los tramos cambian: se olvida la selección.
        ctx.data_mut(|data| data.remove::<(Wall, usize)>(selected_id()));
        store(&ctx, segments);
    }
}

/// Solo para las capturas: `HMF_SEGMENTS="shroud:0.3,0.6:1"` corta la tapa
/// delantera en `t = 0,3` y `0,6` y pone en recta el tramo 1. Varias tapas
/// se separan con `;`.
fn seed_from_env(ctx: &egui::Context) {
    let seeded = egui::Id::unique("meridian_segments_seeded");
    if ctx.data(|data| data.get_temp::<bool>(seeded)).is_some() {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(seeded, true));
    let Ok(text) = std::env::var("HMF_SEGMENTS") else {
        return;
    };
    let mut segments = Segments::default();
    for part in text.split(';').filter(|p| !p.trim().is_empty()) {
        let mut fields = part.split(':');
        let wall = match fields.next().map(str::trim) {
            Some("shroud") => Wall::Shroud,
            Some("hub") => Wall::Hub,
            other => {
                eprintln!("HMF_SEGMENTS: no entiendo la tapa {other:?}");
                continue;
            }
        };
        let state = segments.wall_mut(wall);
        for t in fields
            .next()
            .unwrap_or("")
            .split(',')
            .filter_map(|t| t.trim().parse().ok())
        {
            state.cut(t);
        }
        for index in fields
            .next()
            .unwrap_or("")
            .split(',')
            .filter_map(|i| i.trim().parse::<usize>().ok())
        {
            if let Some(straight) = state.straight.get_mut(index) {
                *straight = true;
            }
        }
    }
    store(ctx, segments);
}

#[cfg(test)]
mod tests {
    use super::*;

    const POINTS: [[f64; 2]; 5] = [
        [0.0, 50.0],
        [10.0, 52.0],
        [30.0, 70.0],
        [38.0, 95.0],
        [40.0, 120.0],
    ];

    #[test]
    fn de_casteljau_hits_the_ends() {
        assert_eq!(point_at(&POINTS, 0.0), POINTS[0]);
        assert_eq!(point_at(&POINTS, 1.0), POINTS[4]);
    }

    #[test]
    fn nearest_t_finds_a_point_on_the_curve() {
        let at = point_at(&POINTS, 0.37);
        let (t, d) = nearest_t(&POINTS, at, |a, b| (a[0] - b[0]).hypot(a[1] - b[1]));
        assert!((t - 0.37).abs() < 1e-6, "t = {t}");
        assert!(d < 1e-6);
    }

    #[test]
    fn cut_pieces_draw_the_same_curve() {
        let mut state = WallSegments::default();
        assert!(state.cut(0.4));
        assert!(state.cut(0.7));
        let pieces = state.contour(&POINTS);
        assert_eq!(pieces.len(), 3);
        // Los tramos se tocan en los cortes y cada punto está sobre la Bézier.
        assert_eq!(pieces[0].last(), pieces[1].first());
        assert_eq!(pieces[1].last(), pieces[2].first());
        assert_eq!(pieces[0][0], POINTS[0]);
        assert_eq!(*pieces[2].last().unwrap(), POINTS[4]);
    }

    #[test]
    fn straight_piece_is_the_chord() {
        let mut state = WallSegments::default();
        state.cut(0.3);
        state.cut(0.6);
        state.straight[1] = true;
        let pieces = state.contour(&POINTS);
        assert_eq!(
            pieces[1],
            vec![point_at(&POINTS, 0.3), point_at(&POINTS, 0.6)]
        );
    }

    #[test]
    fn refuses_cuts_too_close_or_on_a_straight() {
        let mut state = WallSegments::default();
        assert!(!state.cut(0.001));
        assert!(state.cut(0.5));
        assert!(!state.cut(0.502));
        state.straight[1] = true;
        assert!(!state.cut(0.8));
        assert!(state.cut(0.2));
    }

    #[test]
    fn removing_a_cut_merges_the_pieces() {
        let mut state = WallSegments::default();
        state.cut(0.3);
        state.cut(0.6);
        state.straight[1] = true;
        state.remove_cut(0);
        assert_eq!(state.cuts, vec![0.6]);
        assert_eq!(state.straight, vec![false, false]);
        state.remove_cut(0);
        assert_eq!(state, WallSegments::default());
    }
}
