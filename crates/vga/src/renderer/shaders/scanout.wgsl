// Frame-constant register decoding and pixel-format dispatch.
// Captured registers are uniform across the triangle. Decode their constant
// fields at the three vertices and pass them flat to every pixel in this draw.
// Group scalars into vectors to stay within WebGPU's inter-stage variable limit.
struct DisplayVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) grid: vec4<u32>, // Columns, clock width, row height, count divisor.
    @location(1) @interpolate(flat) addressing: vec4<u32>, // Start, stride, mode, underline.
    @location(2) @interpolate(flat) panning: vec4<u32>, // Preset row, byte/pel pan, line compare.
    @location(3) @interpolate(flat) fonts: vec3<u32>, // A/B bases and dual-font selection.
    @location(4) @interpolate(flat) cursor: vec3<u32>, // Address and start/end scanlines.
    @location(5) @interpolate(flat) scanlines: vec2<u32>, // Scan repeat and output row step.
    @location(6) @interpolate(flat) display: vec4<u32>, // Attribute mode, blink, blanked, eight-bit graphics.
    @location(7) @interpolate(flat) colors0: vec4<u32>,
    @location(8) @interpolate(flat) colors1: vec4<u32>,
    @location(9) @interpolate(flat) colors2: vec4<u32>,
    @location(10) @interpolate(flat) colors3: vec4<u32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> DisplayVertex {
    let positions = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    let clocking = sequencer(1u);
    let attribute_mode = attribute_reg(0x10u);
    let maximum_scan_line = crtc(9u);
    let graphics_mode = (attribute_mode & 1u) != 0u;
    let indexed256 = graphics_mode && (attribute_mode & 0x40u) != 0u
        && (graphics(5u) & 0x40u) != 0u;
    // GC shift bit 5 selects the separate CGA shifter, not four-plane pixels.
    let planar16 = graphics_mode && (attribute_mode & 0x40u) == 0u
        && (graphics(5u) & 0x60u) == 0u;
    let glyph_width = select(9u, 8u, (clocking & 1u) != 0u);

    let line_compare = crtc(0x18u) | ((crtc(7u) & 0x10u) << 4u)
        | ((maximum_scan_line & 0x40u) << 3u);
    let preset_row_scan = crtc(8u);
    var pel_pan = attribute_reg(0x13u);
    if indexed256 {
        pel_pan = (pel_pan & 7u) / 2u;
    } else if !graphics_mode && glyph_width == 9u {
        pel_pan = select(0u, pel_pan + 1u, pel_pan < 8u);
    } else {
        pel_pan &= 7u;
    }

    let address_mode = crtc(0x17u);
    let underline = crtc(0x14u);
    let count_by_two = (address_mode & 8u) != 0u;
    let count_by_four = (underline & 0x20u) != 0u;
    let count_divisor = select(select(1u, 2u, count_by_two), 4u, count_by_four);

    let font_maps = sequencer(3u);
    let extended_memory = (sequencer(4u) & 2u) != 0u;
    let font_mask = select(0x3fffu, 0xffffu, extended_memory);
    let font_a = ((((font_maps >> 2u) & 3u) * 0x4000u)
        + (((font_maps >> 5u) & 1u) * 0x2000u)) & font_mask;
    let font_b = (((font_maps & 3u) * 0x4000u)
        + (((font_maps >> 4u) & 1u) * 0x2000u)) & font_mask;

    let blanked = (clocking & 0x20u) != 0u || (state.attribute_address & 0x20u) == 0u
        || (graphics_mode && !indexed256 && !planar16)
        || (!graphics_mode && (graphics(6u) & 1u) != 0u);
    var colors: array<vec4<u32>, 4>;
    for (var color = 0u; color < 16u; color++) {
        colors[color / 4u][color % 4u] = attribute_color(color);
    }

    var output: DisplayVertex;
    output.position = vec4(positions[index], 0.0, 1.0);
    output.grid = vec4(crtc(1u) + 1u, glyph_width, (maximum_scan_line & 31u) + 1u, count_divisor);
    output.addressing = vec4((crtc(12u) << 8u) | crtc(13u), crtc(0x13u) * 2u, address_mode, underline);
    output.panning = vec4(preset_row_scan & 31u, (preset_row_scan >> 5u) & 3u, pel_pan, line_compare);
    output.fonts = vec3(font_a, font_b, u32(extended_memory && font_a != font_b));
    output.cursor = vec3((crtc(14u) << 8u) | crtc(15u), crtc(10u), crtc(11u));
    output.scanlines = vec2(1u << (maximum_scan_line >> 7u), state.scanline_step);
    output.display = vec4(attribute_mode, state.blink_on, u32(blanked), u32(indexed256));
    output.colors0 = colors[0];
    output.colors1 = colors[1];
    output.colors2 = colors[2];
    output.colors3 = colors[3];
    return output;
}

@fragment
fn fragment(input: DisplayVertex) -> @location(0) vec4<f32> {
    if input.display.z != 0u {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    let pixel = vec2<u32>(input.position.xy);
    let line = display_scanline(input, pixel.y);
    if input.display.w != 0u {
        return unpack4x8unorm(indexed256_color(input, line, pixel.x));
    }
    if (input.display.x & 1u) != 0u {
        return unpack4x8unorm(planar16_color(input, line, pixel.x));
    }
    return unpack4x8unorm(text_color(input, line, pixel.x));
}
