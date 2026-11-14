// Only DAC lookup and live blink selection remain at vertices. All register
// interpretation comes from the card's prepared display parameters.
struct DisplayVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) colors0: vec4<u32>,
    @location(1) @interpolate(flat) colors1: vec4<u32>,
    @location(2) @interpolate(flat) colors2: vec4<u32>,
    @location(3) @interpolate(flat) colors3: vec4<u32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> DisplayVertex {
    var output: DisplayVertex;
    output.position = fullscreen_position(index);
    var colors: array<vec4<u32>, 4>;
    for (var color = 0u; color < 16u; color++) {
        colors[color / 4u][color % 4u] = attribute_color(color);
    }
    output.colors0 = colors[0];
    output.colors1 = colors[1];
    output.colors2 = colors[2];
    output.colors3 = colors[3];
    return output;
}

@fragment
fn fragment(input: DisplayVertex) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(input.position.xy);
    if state.blanked != 0u || pixel_blanked(pixel) {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    let line = display_scanline(pixel.y);
    if state.mode == TEXT {
        return unpack4x8unorm(text_color(input, line, pixel.x));
    }
    if state.mode == INDEXED256 {
        return unpack4x8unorm(indexed256_color(line, pixel.x));
    }
    return unpack4x8unorm(graphics16_color(input, line, pixel.x));
}
