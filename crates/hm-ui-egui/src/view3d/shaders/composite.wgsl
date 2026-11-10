// Composición de las transparentes sobre el color opaco ya resuelto.

@group(0) @binding(0) var accum_texture: texture_2d<f32>;
@group(0) @binding(1) var reveal_texture: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    // Un triángulo que cubre toda la pantalla.
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let reveal = textureLoad(reveal_texture, pixel, 0).r;
    if reveal >= 0.999 {
        discard;
    }
    let accum = textureLoad(accum_texture, pixel, 0);
    return vec4<f32>(accum.rgb / max(accum.a, 1e-5), 1.0 - reveal);
}
