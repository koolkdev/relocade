struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let positions = array(
        vec2(-1.0, -1.0),
        vec2(3.0, -1.0),
        vec2(-1.0, 3.0),
    );
    let p = positions[index];
    return Vertex(vec4(p, 0.0, 1.0), vec2((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5));
}

@group(0) @binding(0) var pixels: texture_2d<f32>;
@group(0) @binding(1) var nearest: sampler;

@fragment
fn fragment(v: Vertex) -> @location(0) vec4<f32> {
    return textureSample(pixels, nearest, v.uv);
}

// Stored image colors are sRGB values. An sRGB output attachment encodes the
// shader result, so decode here to preserve the original displayed colors.
@fragment
fn fragment_srgb(v: Vertex) -> @location(0) vec4<f32> {
    let color = textureSample(pixels, nearest, v.uv);
    let linear = select(
        pow((color.rgb + vec3(0.055)) / 1.055, vec3(2.4)),
        color.rgb / 12.92,
        color.rgb <= vec3(0.04045),
    );
    return vec4(linear, color.a);
}
