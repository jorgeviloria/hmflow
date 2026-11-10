//! Cámara del visor 3D, matemática pura: pivote, orientación, escala y
//! proyección. Unidades de la escena: mm. Píxeles de pantalla con y hacia
//! abajo, como egui.

use glam::{Mat3, Mat4, Quat, Vec3};

/// Proyección del visor; ortográfica por defecto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Projection {
    Orthographic,
    Perspective,
}

/// Las vistas con nombre de la barra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StandardView {
    /// Ojo en −z mirando a +z, y arriba: la vista en planta de siempre.
    FromSuction,
    /// Ojo en +z mirando a −z, y arriba.
    FromHub,
    /// Mirando a +x, y arriba: el corte meridional con z a la derecha.
    Lateral,
    /// Mirando a (−1, −1, 1), con arriba lo más cerca posible de +y.
    Isometric,
}

/// Radianes de órbita por píxel arrastrado.
pub(crate) const ORBIT_RAD_PER_PX: f32 = 0.008;
/// Topes de la media altura visible, en mm.
pub(crate) const HALF_HEIGHT_RANGE_MM: (f32, f32) = (0.5, 5000.0);
/// Holgura de «Encuadrar»: la caja ocupa 1/1,1 del cuadro.
pub(crate) const FIT_MARGIN: f32 = 1.1;
/// Campo de visión vertical en perspectiva.
pub(crate) const PERSPECTIVE_FOV_DEG: f32 = 30.0;
/// Profundidad que se ve delante y detrás del pivote, en mm: cabe holgada
/// cualquier bomba y con profundidad de 24 bits da micras.
const SCENE_DEPTH_MM: f32 = 10_000.0;
/// Media altura de una cámara recién hecha, en mm.
const DEFAULT_HALF_HEIGHT_MM: f32 = 100.0;

/// Cámara que gira alrededor de un pivote. `half_height` es la media altura
/// visible en el plano del pivote, en mm, en las dos proyecciones.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Camera {
    pub pivot: Vec3,
    /// Lleva los ejes de la cámara al mundo: x a la derecha, y arriba y −z
    /// hacia delante.
    pub orientation: Quat,
    pub half_height: f32,
    pub projection: Projection,
}

impl Camera {
    /// Una cámara en el origen, ortográfica, mirando como dice `view`.
    pub(crate) fn looking(view: StandardView) -> Self {
        Self {
            pivot: Vec3::ZERO,
            orientation: orientation_of(view),
            half_height: DEFAULT_HALF_HEIGHT_MM,
            projection: Projection::Orthographic,
        }
    }

    /// Cambia hacia dónde mira; conserva pivote, escala y proyección.
    pub(crate) fn set_view(&mut self, view: StandardView) {
        self.orientation = orientation_of(view);
    }

    pub(crate) fn forward(&self) -> Vec3 {
        self.orientation * Vec3::NEG_Z
    }

    pub(crate) fn up(&self) -> Vec3 {
        self.orientation * Vec3::Y
    }

    pub(crate) fn right(&self) -> Vec3 {
        self.orientation * Vec3::X
    }

    /// Gira alrededor del pivote: el arrastre horizontal sobre el eje
    /// vertical de la pantalla y el vertical sobre el horizontal, de modo
    /// que el modelo sigue al puntero.
    pub(crate) fn orbit(&mut self, dx_px: f32, dy_px: f32) {
        if !(dx_px.is_finite() && dy_px.is_finite()) {
            return;
        }
        let yaw = Quat::from_axis_angle(self.up(), -dx_px * ORBIT_RAD_PER_PX);
        let pitch = Quat::from_axis_angle(self.right(), -dy_px * ORBIT_RAD_PER_PX);
        self.orientation = (yaw * pitch * self.orientation).normalize();
    }

    /// Desplaza en el plano de la pantalla: la escena sigue al puntero.
    pub(crate) fn pan(&mut self, dx_px: f32, dy_px: f32, viewport_px: [f32; 2]) {
        if !(dx_px.is_finite() && dy_px.is_finite()) {
            return;
        }
        let mm_per_px = self.mm_per_px(viewport_px);
        self.pivot += (self.up() * dy_px - self.right() * dx_px) * mm_per_px;
    }

    /// Acerca (`factor` > 1) o aleja con el punto bajo el cursor fijo en el
    /// plano del pivote. Un factor que no es un número positivo no hace nada.
    pub(crate) fn zoom_at(&mut self, factor: f32, cursor_px: [f32; 2], viewport_px: [f32; 2]) {
        if !(factor.is_finite() && factor > 0.0 && cursor_px.iter().all(|c| c.is_finite())) {
            return;
        }
        let [width, height] = sized(viewport_px);
        let before = self.mm_per_px(viewport_px);
        let (low, high) = HALF_HEIGHT_RANGE_MM;
        self.half_height = (self.half_height / factor).clamp(low, high);
        let after = self.mm_per_px(viewport_px);
        let offset_x = cursor_px[0] - 0.5 * width;
        let offset_y = 0.5 * height - cursor_px[1];
        self.pivot += (self.right() * offset_x + self.up() * offset_y) * (before - after);
    }

    /// Centra la caja y la mete en el cuadro con [`FIT_MARGIN`]. Sin caja, o
    /// con una caja que no es finita, no cambia nada.
    pub(crate) fn fit(&mut self, bounds: Option<([f32; 3], [f32; 3])>, viewport_px: [f32; 2]) {
        let Some((low, high)) = bounds else {
            return;
        };
        let (low, high) = (Vec3::from(low), Vec3::from(high));
        if !(low.is_finite() && high.is_finite()) {
            return;
        }
        let centre = 0.5 * (low + high);
        let (right, up) = (self.right(), self.up());
        let [width, height] = sized(viewport_px);
        let aspect = width / height;
        let mut needed = 0.0_f32;
        for corner in 0..8 {
            let pick = |axis: usize| {
                if corner & (1 << axis) == 0 {
                    low[axis]
                } else {
                    high[axis]
                }
            };
            let offset = Vec3::new(pick(0), pick(1), pick(2)) - centre;
            needed = needed
                .max(offset.dot(up).abs())
                .max(offset.dot(right).abs() / aspect);
        }
        let (min, max) = HALF_HEIGHT_RANGE_MM;
        self.pivot = centre;
        self.half_height = (needed * FIT_MARGIN).clamp(min, max);
    }

    /// Del mundo (mm) al recorte de wgpu (profundidad de 0 a 1). Siempre
    /// finita: un cuadro de tamaño cero o no finito cuenta como 1 × 1 px.
    pub(crate) fn view_proj(&self, viewport_px: [f32; 2]) -> Mat4 {
        let [width, height] = sized(viewport_px);
        let aspect = width / height;
        let forward = self.forward();
        match self.projection {
            Projection::Orthographic => {
                let eye = self.pivot - forward * SCENE_DEPTH_MM;
                let view = Mat4::look_to_rh(eye, forward, self.up());
                let half_width = self.half_height * aspect;
                let projection = Mat4::orthographic_rh(
                    -half_width,
                    half_width,
                    -self.half_height,
                    self.half_height,
                    0.0,
                    2.0 * SCENE_DEPTH_MM,
                );
                projection * view
            }
            Projection::Perspective => {
                let half_fov = 0.5 * PERSPECTIVE_FOV_DEG.to_radians();
                let distance = self.half_height / half_fov.tan();
                let eye = self.pivot - forward * distance;
                let view = Mat4::look_to_rh(eye, forward, self.up());
                let near = (0.01 * distance).max(0.01);
                let projection =
                    Mat4::perspective_rh(2.0 * half_fov, aspect, near, distance + SCENE_DEPTH_MM);
                projection * view
            }
        }
    }

    /// Milímetros por píxel en el plano del pivote.
    fn mm_per_px(&self, viewport_px: [f32; 2]) -> f32 {
        2.0 * self.half_height / sized(viewport_px)[1]
    }
}

/// El cuadro con cada lado de al menos 1 px y finito.
fn sized(viewport_px: [f32; 2]) -> [f32; 2] {
    viewport_px.map(|side| if side.is_finite() { side.max(1.0) } else { 1.0 })
}

/// La orientación de una vista: derecha = adelante × arriba.
fn orientation_of(view: StandardView) -> Quat {
    let (forward, up) = match view {
        StandardView::FromSuction => (Vec3::Z, Vec3::Y),
        StandardView::FromHub => (Vec3::NEG_Z, Vec3::Y),
        StandardView::Lateral => (Vec3::X, Vec3::Y),
        StandardView::Isometric => {
            let forward = Vec3::new(-1.0, -1.0, 1.0).normalize();
            (
                forward,
                (Vec3::Y - Vec3::Y.dot(forward) * forward).normalize(),
            )
        }
    };
    let right = forward.cross(up);
    Quat::from_mat3(&Mat3::from_cols(right, up, -forward)).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Vec3, Vec4};

    const VIEWPORT: [f32; 2] = [800.0, 600.0];

    fn close(a: Vec3, b: Vec3, what: &str) {
        assert!((a - b).length() < 1e-6, "{what}: {a} ≠ {b}");
    }

    /// De mundo a píxel, con y hacia abajo.
    fn to_pixel(camera: &Camera, point: Vec3, viewport: [f32; 2]) -> [f32; 2] {
        let clip = camera.view_proj(viewport) * point.extend(1.0);
        let ndc = clip / clip.w;
        [
            (ndc.x + 1.0) * 0.5 * viewport[0],
            (1.0 - ndc.y) * 0.5 * viewport[1],
        ]
    }

    fn finite(camera: &Camera) -> bool {
        camera.pivot.is_finite() && camera.orientation.is_finite() && camera.half_height.is_finite()
    }

    #[test]
    fn the_standard_views_look_where_they_say() {
        let cases = [
            (StandardView::FromSuction, Vec3::Z, Vec3::Y, -Vec3::X),
            (StandardView::FromHub, -Vec3::Z, Vec3::Y, Vec3::X),
            (StandardView::Lateral, Vec3::X, Vec3::Y, Vec3::Z),
        ];
        for (view, forward, up, right) in cases {
            let camera = Camera::looking(view);
            close(camera.forward(), forward, &format!("{view:?} adelante"));
            close(camera.up(), up, &format!("{view:?} arriba"));
            close(camera.right(), right, &format!("{view:?} derecha"));
        }
        let iso = Camera::looking(StandardView::Isometric);
        let forward = Vec3::new(-1.0, -1.0, 1.0).normalize();
        close(iso.forward(), forward, "isométrica adelante");
        let up = (Vec3::Y - Vec3::Y.dot(forward) * forward).normalize();
        close(iso.up(), up, "isométrica arriba");
        close(iso.right(), forward.cross(up), "isométrica derecha");
    }

    #[test]
    fn set_view_keeps_pivot_and_scale() {
        let mut camera = Camera::looking(StandardView::Isometric);
        camera.pivot = Vec3::new(1.0, 2.0, 3.0);
        camera.half_height = 42.0;
        camera.set_view(StandardView::Lateral);
        assert_eq!(camera.pivot, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(camera.half_height, 42.0);
        close(camera.forward(), Vec3::X, "lateral");
    }

    #[test]
    fn orbit_keeps_the_pivot_and_the_scale() {
        let mut camera = Camera::looking(StandardView::Isometric);
        camera.pivot = Vec3::new(10.0, -5.0, 30.0);
        camera.half_height = 80.0;
        let before = camera.forward();
        camera.orbit(120.0, -45.0);
        assert_eq!(camera.pivot, Vec3::new(10.0, -5.0, 30.0));
        assert_eq!(camera.half_height, 80.0);
        assert!((camera.forward().length() - 1.0).abs() < 1e-6);
        assert!((camera.forward() - before).length() > 0.1);
        // Arriba y derecha siguen formando una base ortonormal.
        assert!(camera.up().dot(camera.forward()).abs() < 1e-5);
        assert!(camera.right().dot(camera.up()).abs() < 1e-5);
    }

    #[test]
    fn pan_moves_the_scene_with_the_pointer() {
        let mut camera = Camera::looking(StandardView::FromSuction);
        let point = Vec3::new(20.0, 10.0, 5.0);
        let before = to_pixel(&camera, point, VIEWPORT);
        camera.pan(30.0, -12.0, VIEWPORT);
        let after = to_pixel(&camera, point, VIEWPORT);
        assert!((after[0] - before[0] - 30.0).abs() < 1e-3, "{after:?}");
        assert!((after[1] - before[1] + 12.0).abs() < 1e-3, "{after:?}");
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let mut camera = Camera::looking(StandardView::Isometric);
        camera.pivot = Vec3::new(5.0, 5.0, 5.0);
        let cursor = [610.0, 140.0];
        // El punto del mundo bajo el cursor, en el plano del pivote. Se saca
        // con los ejes de la cámara: invertir la matriz en f32 pierde más de
        // 1e-3 px con 20 m de profundidad.
        let mm_per_px = 2.0 * camera.half_height / VIEWPORT[1];
        let world = camera.pivot
            + camera.right() * (cursor[0] - 0.5 * VIEWPORT[0]) * mm_per_px
            + camera.up() * (0.5 * VIEWPORT[1] - cursor[1]) * mm_per_px;
        let pixel = to_pixel(&camera, world, VIEWPORT);
        assert!((pixel[0] - cursor[0]).abs() < 1e-3 && (pixel[1] - cursor[1]).abs() < 1e-3);
        for factor in [1.25, 0.6] {
            camera.zoom_at(factor, cursor, VIEWPORT);
            let pixel = to_pixel(&camera, world, VIEWPORT);
            assert!((pixel[0] - cursor[0]).abs() < 1e-3, "{pixel:?}");
            assert!((pixel[1] - cursor[1]).abs() < 1e-3, "{pixel:?}");
        }
    }

    #[test]
    fn zoom_stops_at_the_limits() {
        let (low, high) = HALF_HEIGHT_RANGE_MM;
        let mut camera = Camera::looking(StandardView::Lateral);
        camera.zoom_at(1e9, [400.0, 300.0], VIEWPORT);
        assert!(camera.half_height >= low && camera.half_height <= high);
        camera.zoom_at(1e-9, [400.0, 300.0], VIEWPORT);
        assert!(camera.half_height >= low && camera.half_height <= high);
        let before = camera.clone();
        for factor in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            camera.zoom_at(factor, [400.0, 300.0], VIEWPORT);
            assert_eq!(camera, before, "factor {factor}");
        }
    }

    #[test]
    fn fit_puts_the_box_inside_the_view() {
        let low = [-100.0, -100.0, 0.0];
        let high = [100.0, 100.0, 60.0];
        for view in [
            StandardView::FromSuction,
            StandardView::FromHub,
            StandardView::Lateral,
            StandardView::Isometric,
        ] {
            let mut camera = Camera::looking(view);
            camera.fit(Some((low, high)), VIEWPORT);
            let view_proj = camera.view_proj(VIEWPORT);
            let mut largest = 0.0_f32;
            for corner in 0..8 {
                let pick = |axis: usize| {
                    if corner & (1 << axis) == 0 {
                        low[axis]
                    } else {
                        high[axis]
                    }
                };
                let clip = view_proj * Vec4::new(pick(0), pick(1), pick(2), 1.0);
                let ndc = clip / clip.w;
                assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0, "{view:?}: {ndc}");
                assert!((0.0..=1.0).contains(&ndc.z), "{view:?}: profundidad {ndc}");
                largest = largest.max(ndc.x.abs()).max(ndc.y.abs());
            }
            assert!(largest >= 1.0 / FIT_MARGIN - 0.05, "{view:?}: {largest}");
        }
    }

    #[test]
    fn fit_ignores_an_empty_scene() {
        let mut camera = Camera::looking(StandardView::Isometric);
        let before = camera.clone();
        camera.fit(None, VIEWPORT);
        assert_eq!(camera, before);
        camera.fit(Some(([f32::NAN, 0.0, 0.0], [1.0, 1.0, 1.0])), VIEWPORT);
        assert_eq!(camera, before);
        // Un solo punto: la cámara se centra en él, sin NaN.
        camera.fit(Some(([3.0, 4.0, 5.0], [3.0, 4.0, 5.0])), [0.0, 0.0]);
        assert!(finite(&camera));
        assert_eq!(camera.pivot, Vec3::new(3.0, 4.0, 5.0));
    }

    #[test]
    fn a_zero_sized_view_gives_a_finite_matrix() {
        for projection in [Projection::Orthographic, Projection::Perspective] {
            let mut camera = Camera::looking(StandardView::Isometric);
            camera.projection = projection;
            for viewport in [[0.0, 0.0], [800.0, 0.0], [0.0, 600.0], [f32::NAN, 600.0]] {
                let matrix = camera.view_proj(viewport);
                assert!(matrix.is_finite(), "{projection:?} {viewport:?}");
                camera.pan(10.0, 10.0, viewport);
                camera.zoom_at(1.1, [0.0, 0.0], viewport);
                assert!(finite(&camera), "{projection:?} {viewport:?}");
            }
        }
    }

    #[test]
    fn from_suction_matches_the_plan_view() {
        // La planta dibuja (r·cos a, r·sin a); la escena pone ese punto en
        // (−r·cos a, r·sin a, z).
        let camera = Camera::looking(StandardView::FromSuction);
        let scale = 0.5 * VIEWPORT[1] / camera.half_height;
        for (r, a) in [(50.0_f32, 0.3_f32), (80.0, 2.0), (20.0, -1.2)] {
            let point = Vec3::new(-r * a.cos(), r * a.sin(), 7.0);
            let pixel = to_pixel(&camera, point, VIEWPORT);
            let x = (pixel[0] - 0.5 * VIEWPORT[0]) / scale;
            let y = (0.5 * VIEWPORT[1] - pixel[1]) / scale;
            assert!((x - r * a.cos()).abs() < 1e-3, "{x}");
            assert!((y - r * a.sin()).abs() < 1e-3, "{y}");
        }
    }

    #[test]
    fn perspective_is_finite_and_keeps_the_pivot_centred() {
        let mut camera = Camera::looking(StandardView::Isometric);
        camera.projection = Projection::Perspective;
        camera.pivot = Vec3::new(12.0, -7.0, 40.0);
        camera.half_height = 150.0;
        let matrix = camera.view_proj(VIEWPORT);
        assert!(matrix.is_finite());
        let pixel = to_pixel(&camera, camera.pivot, VIEWPORT);
        assert!((pixel[0] - 400.0).abs() < 1e-2 && (pixel[1] - 300.0).abs() < 1e-2);
        // En el plano del pivote, la altura visible es la misma que en
        // ortográfica.
        let top = camera.pivot + camera.up() * camera.half_height;
        assert!(to_pixel(&camera, top, VIEWPORT)[1].abs() < 1e-2);
        camera.orbit(50.0, 20.0);
        assert!(camera.view_proj(VIEWPORT).is_finite());
    }
}
