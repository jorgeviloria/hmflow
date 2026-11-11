//! Visor 3D: cámara, gestos y dibujo con wgpu. La vista no calcula
//! geometría: todo vértice sale de `hm_design::scene`.

pub(crate) mod camera;
pub(crate) mod input;
pub(crate) mod renderer;

use eframe::egui;
use eframe::egui_wgpu;
use hm_design::scene::{
    BladeSurfaceSource, PieceId, SCENE_TOLERANCE_FINE_MM, Scene, SceneNotice, scene_from,
};

use crate::model::{Inputs, Outcome};
use crate::theme;
use camera::{Camera, StandardView};
use input::{Gesture, PointerState, gesture};
use renderer::{PieceStyle, Renderer3d};

/// Zoom por línea de rueda.
const ZOOM_PER_LINE: f32 = 1.1;

/// La escena del diseño en pantalla, con el meridiano que dibuja la pestaña
/// Meridiano. `Err` solo si no hay meridiano.
pub(crate) fn scene_of(outcome: &Outcome, tolerance_mm: f64) -> Result<Scene, String> {
    let edits = hm_engine::meridian_edits_of(&outcome.inputs.meridian);
    let (model, cut_error) = crate::meridian::model_or_uncut(&outcome.result, &edits)
        .map_err(|error| format!("no hay meridiano: {error}"))?;
    // Los textos son los de `hm_engine::scene`: la huella incluye los avisos.
    let camber = match crate::blade_inlet::camber_of(outcome) {
        Ok(Some(camber)) => Ok(camber),
        Ok(None) => Err("la superficie de la forma 2D llega en la tarea siguiente".to_string()),
        Err(error) => Err(format!("la línea media del álabe no sale: {error}")),
    };
    let blade = match &camber {
        Ok(camber) => BladeSurfaceSource::Camber(camber),
        Err(message) => BladeSurfaceSource::Unavailable(message.clone()),
    };
    let mut scene = scene_from(
        &model,
        blade,
        outcome.result.selection.blade_count,
        tolerance_mm,
    );
    if let Some(error) = cut_error {
        scene.notices.insert(
            0,
            SceneNotice {
                piece: PieceId::Hub,
                message: format!(
                    "el recorte del cubo no se pudo aplicar ({error}); se dibuja el cubo sin recorte"
                ),
            },
        );
    }
    Ok(scene)
}

/// El visor: estado de cámara, escena vigente y el dibujo.
pub(crate) struct View3d {
    renderer: Option<Renderer3d>,
    /// Por qué no hay dibujo, si no lo hay.
    pub(crate) gpu_error: Option<String>,
    camera: Camera,
    /// Ya se encuadró la primera escena: un cambio de diseño no mueve la cámara.
    fitted: bool,
    scene: Option<Result<Scene, String>>,
    /// Las entradas de las que salió `scene`.
    inputs: Option<Inputs>,
    /// Los estilos con que se subió la escena; cambian con el tema.
    styles: Option<[PieceStyle; 3]>,
}

impl View3d {
    pub(crate) fn new(state: Option<&egui_wgpu::RenderState>) -> Self {
        let (renderer, gpu_error) = match state {
            Some(state) => (Some(Renderer3d::new(state)), None),
            None => (
                None,
                Some("eframe no arrancó con wgpu y no hay dispositivo gráfico".to_string()),
            ),
        };
        Self {
            renderer,
            gpu_error,
            camera: Camera::looking(StandardView::Isometric),
            fitted: false,
            scene: None,
            inputs: None,
            styles: None,
        }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, outcome: &Outcome) {
        let Some(renderer) = self.renderer.as_mut() else {
            let reason = self.gpu_error.as_deref().unwrap_or("motivo desconocido");
            ui.label(format!(
                "El visor 3D no ha podido iniciar el motor gráfico: {reason}. \
                 El resto de HidroMac Flow funciona igual."
            ));
            return;
        };
        let mut upload = false;
        if self.inputs.as_ref() != Some(&outcome.inputs) {
            self.scene = Some(scene_of(outcome, SCENE_TOLERANCE_FINE_MM));
            self.inputs = Some(outcome.inputs.clone());
            upload = true;
        }
        let palette = theme::palette(ui.ctx());
        let styles = styles_of(palette);
        if self.styles != Some(styles) {
            self.styles = Some(styles);
            upload = true;
        }
        if upload {
            let empty = Scene {
                pieces: Vec::new(),
                notices: Vec::new(),
                tolerance: SCENE_TOLERANCE_FINE_MM,
            };
            let scene = match &self.scene {
                Some(Ok(scene)) => scene,
                _ => &empty,
            };
            renderer.set_scene(scene, &|id| style_of(&styles, id));
        }

        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let viewport = [rect.width(), rect.height()];
        let bounds = match &self.scene {
            Some(Ok(scene)) => scene.bounds().map(|(low, high)| {
                (
                    low.map(|value| value as f32),
                    high.map(|value| value as f32),
                )
            }),
            _ => None,
        };
        if !self.fitted && bounds.is_some() {
            self.camera.fit(bounds, viewport);
            self.fitted = true;
        }
        apply_gesture(ui, &response, rect, &mut self.camera, bounds);

        let pixels_per_point = ui.ctx().pixels_per_point();
        let size_px = [
            (rect.width() * pixels_per_point).round().max(0.0) as u32,
            (rect.height() * pixels_per_point).round().max(0.0) as u32,
        ];
        let background = egui::Rgba::from(palette.background).to_array();
        if let Some(texture) = renderer.paint(size_px, &self.camera, &|_| true, background) {
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        let messages: Vec<String> = match &self.scene {
            Some(Ok(scene)) => scene
                .notices
                .iter()
                .map(|notice| format!("{}: {}", notice.piece.label(), notice.message))
                .collect(),
            Some(Err(error)) => vec![error.clone()],
            None => Vec::new(),
        };
        let mut at = rect.left_bottom() + egui::vec2(8.0, -8.0);
        for message in messages.iter().rev() {
            let galley = ui.painter().layout_no_wrap(
                message.clone(),
                egui::FontId::proportional(12.0),
                palette.text_weak,
            );
            at.y -= galley.size().y;
            ui.painter().galley(at, galley, palette.text_weak);
        }
    }
}

/// Colores lineales de cubo, tapa y álabes con el tema vigente.
fn styles_of(palette: &theme::Palette) -> [PieceStyle; 3] {
    let linear = |color: egui::Color32, alpha: f32| {
        let [r, g, b, _] = egui::Rgba::from(color).to_array();
        PieceStyle {
            color: [r, g, b, alpha],
        }
    };
    [
        linear(palette.text_weak, 1.0),
        linear(palette.text_weak, 0.35),
        linear(palette.accent, 1.0),
    ]
}

fn style_of(styles: &[PieceStyle; 3], id: PieceId) -> PieceStyle {
    match id {
        PieceId::Hub => styles[0],
        PieceId::Shroud => styles[1],
        PieceId::BladeMean { .. } => styles[2],
    }
}

/// Lee ratón y trackpad sobre el visor y mueve la cámara.
fn apply_gesture(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    camera: &mut Camera,
    bounds: Option<([f32; 3], [f32; 3])>,
) {
    let viewport = [rect.width(), rect.height()];
    let hovered = response.contains_pointer();
    let mut state = PointerState {
        middle_down: response.dragged_by(egui::PointerButton::Middle),
        middle_double_click: response.double_clicked_by(egui::PointerButton::Middle),
        ..PointerState::default()
    };
    let mut trackpad = egui::Vec2::ZERO;
    ui.input(|input| {
        state.ctrl = input.modifiers.ctrl || input.modifiers.command;
        state.shift = input.modifiers.shift;
        if !hovered {
            return;
        }
        for event in &input.events {
            if let egui::Event::MouseWheel { unit, delta, .. } = event {
                match unit {
                    egui::MouseWheelUnit::Point => trackpad += *delta,
                    egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                        state.wheel_lines += delta.y;
                    }
                }
            }
        }
        state.pinch = input.zoom_delta();
    });
    state.trackpad_scroll = [trackpad.x, trackpad.y];
    // El arrastre con el botón central, o los dos dedos si no lo hay.
    let motion = if state.middle_down {
        response.drag_delta()
    } else {
        trackpad
    };
    match gesture(&state) {
        Gesture::None => {}
        Gesture::Fit => camera.fit(bounds, viewport),
        Gesture::Orbit => camera.orbit(motion.x, motion.y),
        Gesture::Pan => camera.pan(motion.x, motion.y, viewport),
        Gesture::Zoom => {
            let cursor = response.hover_pos().unwrap_or(rect.center()) - rect.min;
            let factor = state.pinch * ZOOM_PER_LINE.powf(state.wheel_lines);
            camera.zoom_at(factor, [cursor.x, cursor.y], viewport);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Basis, Inputs, compute};
    use hm_design::scene::{PieceId, SCENE_TOLERANCE_FINE_MM};

    fn outcome() -> crate::model::Outcome {
        compute(&Inputs::default(), &Basis::default()).unwrap()
    }

    #[test]
    fn scene_of_the_reference_outcome_has_hub_and_shroud() {
        let scene = scene_of(&outcome(), SCENE_TOLERANCE_FINE_MM).unwrap();
        assert!(scene.piece(PieceId::Hub).is_some());
        assert!(scene.piece(PieceId::Shroud).is_some());
        assert_eq!(scene.tolerance, SCENE_TOLERANCE_FINE_MM);
    }

    /// La vista y el motor (lo que se exportaría) arman la misma escena, con
    /// álabes libres y reglados.
    #[test]
    fn the_view_and_the_engine_build_the_same_scene() {
        use hm_design::blade_setup::BladeShape;
        for shape in [BladeShape::Free3D, BladeShape::Ruled3D] {
            let mut inputs = Inputs::default();
            inputs.blade.shape = shape;
            inputs.blade.streamlines = 5;
            let basis = Basis::default();
            let outcome = compute(&inputs, &basis).unwrap();
            let view = scene_of(&outcome, SCENE_TOLERANCE_FINE_MM).unwrap();
            assert!(
                view.piece(PieceId::BladeMean { index: 0 }).is_some(),
                "{shape:?}: {:?}",
                view.notices
            );
            let built = crate::model::build_project(&inputs, &basis, None, "rodete").unwrap();
            let engine = hm_engine::scene(&built.project, SCENE_TOLERANCE_FINE_MM).unwrap();
            assert_eq!(view.fingerprint(), engine.fingerprint(), "{shape:?}");
        }
    }

    #[test]
    fn without_a_render_state_the_view_reports_it() {
        let mut view = View3d::new(None);
        assert!(view.gpu_error.is_some());
        let outcome = outcome();
        egui::__run_test_ui(|ui| view.show(ui, &outcome));
    }
}
