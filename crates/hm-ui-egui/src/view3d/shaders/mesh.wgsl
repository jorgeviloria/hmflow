// Mallas del visor 3D: opacas y transparentes (weighted blended OIT).

struct Camera {
    view_proj: mat4x4<f32>,
    // Dirección de la luz: la de la cámara.
    light: vec4<f32>,
    // Ancho y alto del cuadro en píxeles y ancho de curva; lo usa line.wgsl.
    viewport: vec4<f32>,
};

struct Piece {
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> piece: Piece;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>) -> VertexOut {
    var out: VertexOut;
    out.clip = camera.view_proj * vec4<f32>(position, 1.0);
    out.normal = normal;
    return out;
}

// Luz por las dos caras: 0,35 + 0,65·|n·l|.
fn shaded(normal: vec3<f32>) -> vec3<f32> {
    let n = normalize(normal);
    let lambert = abs(dot(n, camera.light.xyz));
    return piece.color.rgb * (0.35 + 0.65 * lambert);
}

@fragment
fn fs_opaque(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(shaded(in.normal), 1.0);
}

struct Transparent {
    @location(0) accum: vec4<f32>,
    @location(1) reveal: f32,
};

@fragment
fn fs_transparent(in: VertexOut) -> Transparent {
    let alpha = piece.color.a;
    let z = in.clip.z;
    let weight = clamp(alpha * max(0.01, 3000.0 * pow(1.0 - z, 3.0)), 0.01, 3000.0);
    var out: Transparent;
    out.accum = vec4<f32>(shaded(in.normal) * alpha, alpha) * weight;
    out.reveal = alpha;
    return out;
}
