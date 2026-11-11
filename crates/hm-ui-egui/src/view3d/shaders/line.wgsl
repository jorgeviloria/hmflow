// Curvas de borde del visor 3D: cada tramo es un cuadrilátero con ancho en
// píxeles de pantalla (las líneas nativas de wgpu son de 1 px).

struct Camera {
    view_proj: mat4x4<f32>,
    light: vec4<f32>,
    // Ancho y alto del cuadro en píxeles y ancho de la curva en píxeles.
    viewport: vec4<f32>,
};

struct Piece {
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> piece: Piece;

// Sesgo de profundidad, en profundidad normalizada: la curva gana a su propia
// superficie sin parpadear. En ortográfica son 0,2 mm.
const DEPTH_BIAS: f32 = 1e-5;

// Del recorte a píxeles con origen en el centro.
fn to_pixels(clip: vec4<f32>, size: vec2<f32>) -> vec2<f32> {
    return clip.xy / max(clip.w, 1e-6) * 0.5 * size;
}

@vertex
fn vs_main(
    @builtin(vertex_index) corner: u32,
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
) -> @builtin(position) vec4<f32> {
    // Dos triángulos: (a−, b−, b+) y (a−, b+, a+).
    var at_b = array<bool, 6>(false, true, true, false, true, false);
    var sides = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let size = max(camera.viewport.xy, vec2<f32>(1.0, 1.0));
    let clip_a = camera.view_proj * vec4<f32>(a, 1.0);
    let clip_b = camera.view_proj * vec4<f32>(b, 1.0);
    var along = to_pixels(clip_b, size) - to_pixels(clip_a, size);
    if (length(along) < 1e-6) {
        along = vec2<f32>(1.0, 0.0);
    }
    let direction = normalize(along);
    let normal = vec2<f32>(-direction.y, direction.x);
    var clip = select(clip_a, clip_b, at_b[corner]);
    let offset_px = normal * sides[corner] * 0.5 * camera.viewport.z;
    clip.x += offset_px.x / (0.5 * size.x) * clip.w;
    clip.y += offset_px.y / (0.5 * size.y) * clip.w;
    clip.z -= DEPTH_BIAS * clip.w;
    return clip;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(piece.color.rgb, 1.0);
}
