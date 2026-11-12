//! Visor 3D: cámara, gestos y dibujo con wgpu. La vista no calcula
//! geometría: todo vértice sale de `hm_design::scene`.

pub(crate) mod camera;
pub(crate) mod input;
pub(crate) mod renderer;

use eframe::egui;
use eframe::egui_wgpu;
use hm_design::scene::{
    BladeSurfaceSource, PieceId, SCENE_TOLERANCE_DRAG_MM, SCENE_TOLERANCE_FINE_MM, Scene,
    SceneNotice, scene_from,
};

use crate::model::{Inputs, Outcome};
use crate::theme;
use camera::{Camera, Projection, StandardView};
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
    let camber = crate::blade_inlet::camber_of(outcome);
    // Con línea media 3D, el álabe con espesor (o el motivo de que no salga).
    let solid = match &camber {
        Ok(Some(_)) => Some(crate::blade_inlet::solid_of(outcome).map_err(|e| e.to_string())),
        _ => None,
    };
    let blade = match (&camber, &outcome.geometry) {
        (Ok(Some(camber)), _) => BladeSurfaceSource::Solid {
            camber,
            solid: solid
                .as_ref()
                .expect("con línea media hay sólido o motivo")
                .as_ref()
                .map_err(Clone::clone),
        },
        (Ok(None), Ok(geometry)) => BladeSurfaceSource::Planar(&geometry.impeller.blade),
        (Ok(None), Err(error)) => {
            BladeSurfaceSource::Unavailable(format!("sin geometría no hay álabes: {error}"))
        }
        (Err(error), _) => {
            BladeSurfaceSource::Unavailable(format!("la línea media del álabe no sale: {error}"))
        }
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

/// Qué piezas se ven: las casillas provisionales de la barra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Visibility {
    pub hub: bool,
    pub shroud: bool,
    pub blades: bool,
    /// La superficie media junto al álabe con espesor. Oculta por defecto.
    pub mean: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self {
            hub: true,
            shroud: true,
            blades: true,
            mean: false,
        }
    }
}

impl Visibility {
    /// `has_solid`: la escena tiene álabes con espesor. Entonces la
    /// superficie media sigue a su casilla; si no (porque el álabe con
    /// espesor no sale), sigue a «Álabes» y se ve como siempre.
    pub(crate) fn shows(&self, id: PieceId, has_solid: bool) -> bool {
        match id {
            PieceId::Hub => self.hub,
            PieceId::Shroud => self.shroud,
            PieceId::Blade { .. } => self.blades,
            PieceId::BladeMean { .. } if has_solid => self.mean,
            PieceId::BladeMean { .. } => self.blades,
        }
    }
}

/// La escena tiene álabes con espesor.
fn has_solid(scene: &Scene) -> bool {
    scene
        .pieces
        .iter()
        .any(|piece| matches!(piece.id, PieceId::Blade { .. }))
}

/// Ancho inicial del panel lateral del visor, en puntos de egui.
pub(crate) const PANEL_DEFAULT_WIDTH: f32 = 420.0;

/// Ancho de las curvas de borde, en puntos de egui.
pub(crate) const CURVE_WIDTH_PX: f32 = 1.5;
/// Lado del triedro, en puntos de egui.
const TRIAD_SIZE_PX: f32 = 48.0;

/// Ejes x, y, z proyectados a la pantalla (y hacia abajo), para el triedro.
/// Un eje que apunta al ojo queda en (0, 0).
pub(crate) fn triad_axes(camera: &Camera) -> [[f32; 2]; 3] {
    let (right, up) = (camera.right(), camera.up());
    [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z].map(|axis| [axis.dot(right), -axis.dot(up)])
}

/// La caja de las piezas que se ven, en f32 para la cámara.
fn visible_bounds(scene: &Scene, visibility: &Visibility) -> Option<([f32; 3], [f32; 3])> {
    let solid = has_solid(scene);
    scene
        .pieces
        .iter()
        .filter(|piece| visibility.shows(piece.id, solid))
        .filter_map(|piece| piece.surface.bounds())
        .reduce(|(low, high), (piece_low, piece_high)| {
            (
                std::array::from_fn(|axis| low[axis].min(piece_low[axis])),
                std::array::from_fn(|axis| high[axis].max(piece_high[axis])),
            )
        })
        .map(|(low, high)| {
            (
                low.map(|value| value as f32),
                high.map(|value| value as f32),
            )
        })
}

/// Una línea por aviso: «{pieza}: {mensaje}».
/// El error de la última escena que no salió va primero.
fn notice_lines(scene: Option<&Scene>, error: Option<&str>) -> Vec<String> {
    error
        .map(str::to_string)
        .into_iter()
        .chain(scene.into_iter().flat_map(|scene| {
            scene
                .notices
                .iter()
                .map(|notice| format!("{}: {}", notice.piece.label(), notice.message))
        }))
        .collect()
}

/// Franja plegable «Avisos (n)»; no se pinta sin avisos. Devuelve su título.
fn notice_strip(ui: &mut egui::Ui, lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let title = format!("Avisos ({})", lines.len());
    egui::CollapsingHeader::new(&title)
        .id_salt("visor 3D: avisos")
        .default_open(false)
        .show(ui, |ui| {
            for line in lines {
                ui.label(line);
            }
        });
    Some(title)
}

/// Lo que se pide desde la barra y se aplica cuando ya se sabe el cuadro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarAction {
    View(StandardView),
    Fit,
}

/// El visor: estado de cámara, escena vigente y el dibujo.
pub(crate) struct View3d {
    renderer: Option<Renderer3d>,
    /// Por qué no hay dibujo, si no lo hay.
    pub(crate) gpu_error: Option<String>,
    camera: Camera,
    /// Ya se encuadró la primera escena: un cambio de diseño no mueve la cámara.
    fitted: bool,
    /// La última escena válida.
    scene: Option<Scene>,
    /// Por qué no salió la escena de las entradas vigentes; se sigue viendo
    /// la última válida.
    scene_error: Option<String>,
    /// Entradas y tolerancia de la última construcción, salga o no.
    built_from: Option<(Inputs, f64)>,
    /// Hay que subir la escena a la tarjeta.
    upload: bool,
    /// Cuántas escenas se han construido; lo leen las pruebas.
    pub(crate) builds: u32,
    /// El visor está abierto en el panel lateral de Geometría y Meridiano.
    pub(crate) panel_open: bool,
    /// Los estilos con que se subió la escena; cambian con el tema y el fondo.
    styles: Option<Styles>,
    visibility: Visibility,
    /// Fondo blanco en vez del del tema; el visor usa entonces los colores
    /// del tema claro.
    white_background: bool,
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
            scene_error: None,
            built_from: None,
            upload: false,
            builds: 0,
            panel_open: false,
            styles: None,
            visibility: Visibility::default(),
            white_background: false,
        }
    }

    /// Rehace la escena si cambió `outcome.inputs` o si toca pasar de la
    /// malla de arrastre a la fina. Mientras se arrastra sin cambiar nada se
    /// conserva la que hay.
    pub(crate) fn sync(&mut self, outcome: &Outcome, dragging: bool) {
        let tolerance = if dragging {
            SCENE_TOLERANCE_DRAG_MM
        } else {
            SCENE_TOLERANCE_FINE_MM
        };
        let rebuild = match &self.built_from {
            None => true,
            Some((inputs, built)) => {
                *inputs != outcome.inputs || (!dragging && *built != tolerance)
            }
        };
        if !rebuild {
            return;
        }
        self.builds += 1;
        self.built_from = Some((outcome.inputs.clone(), tolerance));
        match scene_of(outcome, tolerance) {
            Ok(scene) => {
                self.scene = Some(scene);
                self.scene_error = None;
                self.upload = true;
            }
            Err(error) => self.scene_error = Some(error),
        }
    }

    /// La escena que se ve es la de arrastre y la fina está por llegar. Si
    /// la última no salió no llega nada: eso lo dice [`View3d::stale_note`].
    pub(crate) fn is_preview(&self) -> bool {
        self.scene_error.is_none()
            && self
                .scene
                .as_ref()
                .is_some_and(|scene| scene.tolerance == SCENE_TOLERANCE_DRAG_MM)
    }

    /// Si la escena de las entradas vigentes no salió y se ve la anterior,
    /// la nota que lo dice sobre el lienzo.
    pub(crate) fn stale_note(&self) -> Option<String> {
        self.scene.as_ref()?;
        self.scene_error
            .as_ref()
            .map(|error| format!("escena anterior: {error}"))
    }

    /// Dibuja la última escena de [`View3d::sync`].
    pub(crate) fn show(&mut self, ui: &mut egui::Ui) {
        let Some(renderer) = self.renderer.as_mut() else {
            let reason = self.gpu_error.as_deref().unwrap_or("motivo desconocido");
            ui.label(format!(
                "El visor 3D no ha podido iniciar el motor gráfico: {reason}. \
                 El resto de HidroMac Flow funciona igual."
            ));
            return;
        };
        let mut upload = std::mem::take(&mut self.upload);

        let solid = self.scene.as_ref().is_some_and(has_solid);
        let action = bar(
            ui,
            &mut self.camera,
            &mut self.visibility,
            solid,
            &mut self.white_background,
        );
        notice_strip(
            ui,
            &notice_lines(self.scene.as_ref(), self.scene_error.as_deref()),
        );

        let palette = if self.white_background {
            &theme::LIGHT
        } else {
            theme::palette(ui.ctx())
        };
        let background = if self.white_background {
            theme::LIGHT.paper
        } else {
            palette.background
        };
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
            let scene = self.scene.as_ref().unwrap_or(&empty);
            let solid = has_solid(scene);
            renderer.set_scene(scene, &|id| style_of(&styles, id, solid), styles.curve);
        }

        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let viewport = [rect.width(), rect.height()];
        let bounds = self
            .scene
            .as_ref()
            .and_then(|scene| visible_bounds(scene, &self.visibility));
        if !self.fitted && bounds.is_some() {
            self.camera.fit(bounds, viewport);
            self.fitted = true;
        }
        match action {
            Some(BarAction::View(view)) => self.camera.set_view(view),
            Some(BarAction::Fit) => self.camera.fit(bounds, viewport),
            None => {}
        }
        apply_gesture(ui, &response, rect, &mut self.camera, bounds);

        let pixels_per_point = ui.ctx().pixels_per_point();
        let size_px = [
            (rect.width() * pixels_per_point).round().max(0.0) as u32,
            (rect.height() * pixels_per_point).round().max(0.0) as u32,
        ];
        let visibility = self.visibility;
        let solid = self.scene.as_ref().is_some_and(has_solid);
        if let Some(texture) = renderer.paint(
            size_px,
            &self.camera,
            &|id| visibility.shows(id, solid),
            egui::Rgba::from(background).to_array(),
            CURVE_WIDTH_PX * pixels_per_point,
        ) {
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        paint_triad(ui.painter(), rect, &self.camera, palette);
        let mark = match self.stale_note() {
            Some(note) => Some((note, palette.warn)),
            None => self
                .is_preview()
                .then(|| ("actualizando".to_string(), palette.text_weak)),
        };
        if let Some((text, colour)) = mark {
            let galley = ui.painter().layout(
                text,
                egui::FontId::proportional(12.0),
                colour,
                (rect.width() - 16.0).max(1.0),
            );
            let at = rect.right_top() + egui::vec2(-8.0 - galley.size().x, 8.0);
            ui.painter().galley(at, galley, colour);
        }
    }
}

/// La barra encima de la vista. Las vistas y «Encuadrar» se devuelven para
/// aplicarlas cuando ya se sabe el cuadro.
fn bar(
    ui: &mut egui::Ui,
    camera: &mut Camera,
    visibility: &mut Visibility,
    has_solid: bool,
    white_background: &mut bool,
) -> Option<BarAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        for (label, view) in [
            ("Desde la aspiración", StandardView::FromSuction),
            ("Desde el cubo", StandardView::FromHub),
            ("Lateral", StandardView::Lateral),
            ("Isométrica", StandardView::Isometric),
        ] {
            if ui.button(label).clicked() {
                action = Some(BarAction::View(view));
            }
        }
        if ui.button("Encuadrar").clicked() {
            action = Some(BarAction::Fit);
        }
        ui.separator();
        let mut perspective = camera.projection == Projection::Perspective;
        if ui.toggle_value(&mut perspective, "Perspectiva").changed() {
            camera.projection = if perspective {
                Projection::Perspective
            } else {
                Projection::Orthographic
            };
        }
        ui.separator();
        ui.checkbox(&mut visibility.hub, "Cubo");
        ui.checkbox(&mut visibility.shroud, "Tapa");
        ui.checkbox(&mut visibility.blades, "Álabes");
        // Solo hay superficie media aparte si hay álabe con espesor.
        if has_solid {
            ui.checkbox(&mut visibility.mean, "Superficie media");
        }
        ui.separator();
        ui.toggle_value(white_background, "Fondo blanco");
    });
    action
}

/// El triedro abajo a la izquierda: x rojo, y verde, z azul, con su letra.
fn paint_triad(
    painter: &egui::Painter,
    rect: egui::Rect,
    camera: &Camera,
    palette: &theme::Palette,
) {
    let half = 0.5 * TRIAD_SIZE_PX;
    let centre = rect.left_bottom() + egui::vec2(8.0 + half, -8.0 - half);
    let length = half - 8.0;
    let colours = [palette.axis_x, palette.axis_y, palette.axis_z];
    let letters = ["x", "y", "z"];
    for ((axis, colour), letter) in triad_axes(camera).into_iter().zip(colours).zip(letters) {
        let direction = egui::vec2(axis[0], axis[1]);
        let tip = centre + direction * length;
        painter.line_segment([centre, tip], egui::Stroke::new(2.0, colour));
        painter.text(
            centre + direction * (length + 6.0),
            egui::Align2::CENTER_CENTER,
            letter,
            egui::FontId::proportional(11.0),
            colour,
        );
    }
}

/// Colores lineales de cubo, tapa, álabes, superficie media junto a un sólido
/// y curvas con una paleta.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Styles {
    pieces: [PieceStyle; 4],
    curve: [f32; 4],
}

fn styles_of(palette: &theme::Palette) -> Styles {
    let linear = |color: egui::Color32, alpha: f32| {
        let [r, g, b, _] = egui::Rgba::from(color).to_array();
        [r, g, b, alpha]
    };
    Styles {
        pieces: [
            PieceStyle {
                color: linear(palette.text_weak, 1.0),
            },
            PieceStyle {
                color: linear(palette.text_weak, 0.35),
            },
            PieceStyle {
                color: linear(palette.accent, 1.0),
            },
            // La superficie media junto al álabe con espesor: el mismo acento,
            // translúcido.
            PieceStyle {
                color: linear(palette.accent, 0.35),
            },
        ],
        curve: linear(palette.text, 1.0),
    }
}

fn style_of(styles: &Styles, id: PieceId, has_solid: bool) -> PieceStyle {
    match id {
        PieceId::Hub => styles.pieces[0],
        PieceId::Shroud => styles.pieces[1],
        PieceId::Blade { .. } => styles.pieces[2],
        PieceId::BladeMean { .. } if has_solid => styles.pieces[3],
        PieceId::BladeMean { .. } => styles.pieces[2],
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
    /// álabes libres, reglados y 2D.
    #[test]
    fn the_view_and_the_engine_build_the_same_scene() {
        use hm_design::blade_setup::BladeShape;
        for shape in [BladeShape::Free3D, BladeShape::Ruled3D, BladeShape::Free2D] {
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
    fn visibility_hides_pieces_by_kind() {
        let all = Visibility::default();
        // Sin álabe con espesor, la superficie media sigue a «Álabes», como hoy.
        for id in [
            PieceId::Hub,
            PieceId::Shroud,
            PieceId::BladeMean { index: 4 },
        ] {
            assert!(all.shows(id, false), "{id:?}");
        }
        let no_hub = Visibility {
            hub: false,
            ..Visibility::default()
        };
        assert!(!no_hub.shows(PieceId::Hub, false));
        assert!(no_hub.shows(PieceId::Shroud, false));
        let no_shroud = Visibility {
            shroud: false,
            ..Visibility::default()
        };
        assert!(!no_shroud.shows(PieceId::Shroud, false));
        assert!(no_shroud.shows(PieceId::BladeMean { index: 4 }, false));
        let no_blades = Visibility {
            blades: false,
            ..Visibility::default()
        };
        assert!(!no_blades.shows(PieceId::BladeMean { index: 4 }, false));
        assert!(no_blades.shows(PieceId::Hub, false));
    }

    #[test]
    fn the_triad_follows_the_camera() {
        let camera = Camera::looking(StandardView::FromSuction);
        let expected = [[-1.0, 0.0], [0.0, -1.0], [0.0, 0.0]];
        for (axis, want) in triad_axes(&camera).iter().zip(expected) {
            for (got, want) in axis.iter().zip(want) {
                assert!((got - want).abs() < 1e-6, "{axis:?} ≠ {want:?}");
            }
        }
    }

    /// Una pieza con un solo triángulo entre dos esquinas.
    fn box_piece(id: PieceId, low: [f64; 3], high: [f64; 3]) -> hm_design::scene::Piece {
        hm_design::scene::Piece {
            id,
            surface: hm_geometry::mesh::TriangleMesh {
                vertices: vec![low, [high[0], low[1], low[2]], high],
                normals: vec![[0.0, 0.0, 1.0]; 3],
                triangles: vec![[0, 1, 2]],
            },
            curves: Vec::new(),
            faces: Vec::new(),
        }
    }

    #[test]
    fn fit_uses_only_the_visible_pieces() {
        let scene = Scene {
            pieces: vec![
                box_piece(PieceId::Hub, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]),
                box_piece(
                    PieceId::BladeMean { index: 0 },
                    [-50.0, -50.0, -50.0],
                    [100.0, 100.0, 100.0],
                ),
            ],
            notices: Vec::new(),
            tolerance: SCENE_TOLERANCE_FINE_MM,
        };
        let all = visible_bounds(&scene, &Visibility::default()).unwrap();
        assert_eq!(all, ([-50.0; 3], [100.0; 3]));
        let without_blades = Visibility {
            blades: false,
            ..Visibility::default()
        };
        let hub_only = visible_bounds(&scene, &without_blades).unwrap();
        assert_eq!(hub_only, ([0.0; 3], [10.0; 3]));
        let nothing = Visibility {
            hub: false,
            shroud: false,
            blades: false,
            mean: false,
        };
        assert_eq!(visible_bounds(&scene, &nothing), None);
    }

    #[test]
    fn the_notice_strip_lists_every_notice() {
        let scene = Scene {
            pieces: Vec::new(),
            notices: vec![
                SceneNotice {
                    piece: PieceId::Hub,
                    message: "uno".to_string(),
                },
                SceneNotice {
                    piece: PieceId::BladeMean { index: 1 },
                    message: "dos".to_string(),
                },
            ],
            tolerance: SCENE_TOLERANCE_FINE_MM,
        };
        let lines = notice_lines(Some(&scene), None);
        assert_eq!(lines, ["Cubo: uno", "Superficie media 2: dos"]);
        let mut title = None;
        egui::__run_test_ui(|ui| title = notice_strip(ui, &lines));
        assert_eq!(title.as_deref(), Some("Avisos (2)"));
        let mut none = Some(String::new());
        egui::__run_test_ui(|ui| none = notice_strip(ui, &[]));
        assert_eq!(none, None);
    }

    #[test]
    fn sync_rebuilds_only_when_the_inputs_change() {
        let mut view = View3d::new(None);
        let outcome = outcome();
        view.sync(&outcome, false);
        view.sync(&outcome, false);
        assert_eq!(view.builds, 1);
        let mut changed = outcome.clone();
        changed.inputs.meridian.edge_shroud = "0.1".to_string();
        view.sync(&changed, false);
        assert_eq!(view.builds, 2);
    }

    #[test]
    fn dragging_uses_the_coarse_mesh_and_release_the_fine_one() {
        let mut view = View3d::new(None);
        let outcome = outcome();
        view.sync(&outcome, true);
        assert_eq!(
            view.scene.as_ref().unwrap().tolerance,
            SCENE_TOLERANCE_DRAG_MM
        );
        assert!(view.is_preview());
        view.sync(&outcome, false);
        assert_eq!(
            view.scene.as_ref().unwrap().tolerance,
            SCENE_TOLERANCE_FINE_MM
        );
        assert!(!view.is_preview());
    }

    #[test]
    fn a_failed_scene_keeps_the_last_good_one() {
        let mut view = View3d::new(None);
        let good = outcome();
        view.sync(&good, false);
        let before = view.scene.clone().unwrap();
        let mut bad = good.clone();
        bad.inputs.meridian.hub_diameter = "1000 mm".to_string();
        let error = scene_of(&bad, SCENE_TOLERANCE_FINE_MM).unwrap_err();
        view.sync(&bad, false);
        assert_eq!(view.scene.as_ref(), Some(&before));
        let lines = notice_lines(view.scene.as_ref(), view.scene_error.as_deref());
        assert!(lines.contains(&error), "{lines:?}");
    }

    /// Arrastre que pasa por una zona válida y acaba en una que no sale: al
    /// soltar no queda «actualizando» y el lienzo dice que es la anterior.
    #[test]
    fn a_failed_scene_after_a_drag_is_not_a_preview() {
        let mut view = View3d::new(None);
        let good = outcome();
        view.sync(&good, true);
        assert!(view.is_preview());
        assert_eq!(view.stale_note(), None);
        let mut bad = good.clone();
        bad.inputs.meridian.hub_diameter = "1000 mm".to_string();
        let error = scene_of(&bad, SCENE_TOLERANCE_FINE_MM).unwrap_err();
        view.sync(&bad, true);
        view.sync(&bad, false);
        assert!(!view.is_preview());
        assert_eq!(view.stale_note(), Some(format!("escena anterior: {error}")));
    }

    #[test]
    fn a_new_scene_does_not_move_the_camera() {
        let mut view = View3d::new(None);
        let outcome = outcome();
        view.sync(&outcome, false);
        view.camera.orbit(40.0, -25.0);
        let camera = view.camera.clone();
        let mut changed = outcome.clone();
        changed.inputs.meridian.edge_shroud = "0.1".to_string();
        view.sync(&changed, false);
        assert_eq!(view.builds, 2);
        assert_eq!(view.camera, camera);
    }

    /// Una curva de espesor editada en las dos paredes: nodos (x, e en mm).
    fn edited(nodes: &[(f64, f64)]) -> hm_design::blade_thickness::BladeThickness {
        use hm_design::blade_thickness::{BladeThickness, EditedThickness, ThicknessNode};
        let curve: Vec<ThicknessNode> = nodes
            .iter()
            .map(|&(x, e)| ThicknessNode { x, e: e / 1000.0 })
            .collect();
        BladeThickness {
            same: true,
            edited: Some(EditedThickness {
                hub: curve.clone(),
                shroud: curve,
            }),
        }
    }

    fn thick_outcome(
        shape: hm_design::blade_setup::BladeShape,
        thickness: hm_design::blade_thickness::BladeThickness,
    ) -> (Inputs, Outcome) {
        let mut inputs = Inputs::default();
        inputs.blade.shape = shape;
        inputs.blade.streamlines = 5;
        inputs.blade.thickness = thickness;
        let outcome = compute(&inputs, &Basis::default()).unwrap();
        (inputs, outcome)
    }

    /// La vista y el motor arman la misma escena con una curva de espesor
    /// editada, y con ella sale el álabe con espesor.
    #[test]
    fn the_view_and_the_engine_build_the_same_thick_scene() {
        use hm_design::blade_setup::BladeShape;
        let curve = [(0.0, 6.0), (0.5, 7.0), (1.0, 3.0)];
        for shape in [BladeShape::Free3D, BladeShape::Ruled3D] {
            let (inputs, outcome) = thick_outcome(shape, edited(&curve));
            let view = scene_of(&outcome, SCENE_TOLERANCE_FINE_MM).unwrap();
            assert!(has_solid(&view), "{shape:?}: {:?}", view.notices);
            let built =
                crate::model::build_project(&inputs, &Basis::default(), None, "rodete").unwrap();
            let engine = hm_engine::scene(&built.project, SCENE_TOLERANCE_FINE_MM).unwrap();
            assert_eq!(view.fingerprint(), engine.fingerprint(), "{shape:?}");
        }
    }

    #[test]
    fn the_mean_surface_is_hidden_when_the_blade_has_thickness() {
        let mean = PieceId::BladeMean { index: 0 };
        let blade = PieceId::Blade { index: 0 };
        let by_default = Visibility::default();
        assert!(
            !by_default.mean,
            "la superficie media va oculta por defecto"
        );
        assert!(by_default.shows(blade, true));
        assert!(!by_default.shows(mean, true));
        // La casilla «Superficie media» la enseña junto al sólido.
        let with_mean = Visibility {
            mean: true,
            ..Visibility::default()
        };
        assert!(with_mean.shows(mean, true));
        assert!(with_mean.shows(blade, true));
    }

    #[test]
    fn the_mean_surface_shows_when_the_solid_fails() {
        use hm_design::blade_setup::BladeShape;
        // 60 mm en el nodo central: el álabe con espesor no se construye.
        let (_, outcome) = thick_outcome(
            BladeShape::Free3D,
            edited(&[(0.0, 6.0), (0.5, 60.0), (1.0, 3.0)]),
        );
        let scene = scene_of(&outcome, SCENE_TOLERANCE_FINE_MM).unwrap();
        assert!(!has_solid(&scene));
        assert!(
            scene
                .notices
                .iter()
                .any(|notice| notice.piece == PieceId::Blade { index: 0 }),
            "{:?}",
            scene.notices
        );
        // Sin sólido, la superficie media sigue a «Álabes» y se ve como hoy.
        let visibility = Visibility::default();
        let mean = PieceId::BladeMean { index: 0 };
        assert!(scene.piece(mean).is_some());
        assert!(visibility.shows(mean, has_solid(&scene)));
        let no_blades = Visibility {
            blades: false,
            ..visibility
        };
        assert!(!no_blades.shows(mean, has_solid(&scene)));
    }

    #[test]
    fn fit_ignores_the_hidden_mean_surface() {
        let scene = Scene {
            pieces: vec![
                box_piece(PieceId::Hub, [0.0, 0.0, 0.0], [10.0, 10.0, 10.0]),
                box_piece(
                    PieceId::BladeMean { index: 0 },
                    [-50.0, -50.0, -50.0],
                    [100.0, 100.0, 100.0],
                ),
                box_piece(
                    PieceId::Blade { index: 0 },
                    [1.0, 1.0, 1.0],
                    [20.0, 20.0, 20.0],
                ),
            ],
            notices: Vec::new(),
            tolerance: SCENE_TOLERANCE_FINE_MM,
        };
        let fitted = visible_bounds(&scene, &Visibility::default()).unwrap();
        assert_eq!(fitted, ([0.0; 3], [20.0; 3]));
        let with_mean = Visibility {
            mean: true,
            ..Visibility::default()
        };
        assert_eq!(
            visible_bounds(&scene, &with_mean).unwrap(),
            ([-50.0; 3], [100.0; 3])
        );
    }

    #[test]
    fn the_blades_checkbox_hides_the_thick_blades() {
        let blade = PieceId::Blade { index: 3 };
        let no_blades = Visibility {
            blades: false,
            ..Visibility::default()
        };
        assert!(!no_blades.shows(blade, true));
        assert!(no_blades.shows(PieceId::Hub, true));
        // Con «Álabes» apagado la superficie media sigue a su propia casilla.
        let only_mean = Visibility {
            blades: false,
            mean: true,
            ..Visibility::default()
        };
        assert!(!only_mean.shows(blade, true));
        assert!(only_mean.shows(PieceId::BladeMean { index: 3 }, true));
    }

    /// `cargo test --release scene_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scene_timing() {
        use hm_design::blade_edges::{BladeEdges, LeadingKind, TrailingKind};
        use hm_design::blade_setup::BladeShape;
        let mut inputs = Inputs::default();
        inputs.blade.shape = BladeShape::Free3D;
        let first = compute(&inputs, &Basis::default()).unwrap();
        let r = &first.result;
        let (_, e, _) = hm_design::mechanics::blade_thicknesses(
            r.selection.blade_count,
            r.u2.si,
            r.nq,
            r.d2.si,
        )
        .unwrap();
        let e1 = e / 2.0;
        // Entrada y salida: romo y recortada, la nariz de elipse con la salida
        // limada por succión, y el afilado con la elipse de salida.
        for (kind, tail) in [
            (LeadingKind::Blunt, TrailingKind::Cut),
            (LeadingKind::Ellipse, TrailingKind::Cut),
            (LeadingKind::Wedge, TrailingKind::Cut),
            (LeadingKind::Blunt, TrailingKind::FiledSuction),
            (LeadingKind::Ellipse, TrailingKind::FiledSuction),
            (LeadingKind::Wedge, TrailingKind::Ellipse),
        ] {
            inputs.blade.edges = BladeEdges::default()
                .with_leading_kind(kind, e, e)
                .with_trailing_kind(tail, e1, e1);
            let outcome = compute(&inputs, &Basis::default()).unwrap();
            for (name, tolerance) in [
                ("fina", SCENE_TOLERANCE_FINE_MM),
                ("arrastre", SCENE_TOLERANCE_DRAG_MM),
            ] {
                scene_of(&outcome, tolerance).unwrap();
                let start = std::time::Instant::now();
                for _ in 0..20 {
                    scene_of(&outcome, tolerance).unwrap();
                }
                let ms = start.elapsed().as_secs_f64() * 1000.0 / 20.0;
                println!("scene_of {kind:?}+{tail:?} {name} ({tolerance} mm): {ms:.2} ms");
            }
        }
    }

    #[test]
    fn without_a_render_state_the_view_reports_it() {
        let mut view = View3d::new(None);
        assert!(view.gpu_error.is_some());
        let outcome = outcome();
        view.sync(&outcome, false);
        egui::__run_test_ui(|ui| view.show(ui));
    }
}
