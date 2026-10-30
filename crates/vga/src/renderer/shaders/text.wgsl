// Text-mode register decoding and pixel selection.
// Captured registers are uniform across the triangle. Decode their constant
// fields at the three vertices and pass them flat to every pixel in this draw.
// Group scalars into vectors to stay within WebGPU's inter-stage variable limit.
struct TextVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) grid: vec4<u32>, // Columns, glyph width/height, count divisor.
    @location(1) @interpolate(flat) addressing: vec4<u32>, // Start, stride, mode, underline.
    @location(2) @interpolate(flat) panning: vec4<u32>, // Preset row, byte/pel pan, line compare.
    @location(3) @interpolate(flat) fonts: vec3<u32>, // A/B bases and dual-font selection.
    @location(4) @interpolate(flat) cursor: vec3<u32>, // Address and start/end scanlines.
    @location(5) @interpolate(flat) scanlines: vec2<u32>, // Scan repeat and output row step.
    @location(6) @interpolate(flat) display: vec3<u32>, // Attribute mode, blink, blanked.
    @location(7) @interpolate(flat) colors0: vec4<u32>,
    @location(8) @interpolate(flat) colors1: vec4<u32>,
    @location(9) @interpolate(flat) colors2: vec4<u32>,
    @location(10) @interpolate(flat) colors3: vec4<u32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> TextVertex {
    let positions = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    let clocking = sequencer(1u);
    let attribute_mode = attribute_reg(0x10u);
    let maximum_scan_line = crtc(9u);
    let glyph_width = select(9u, 8u, (clocking & 1u) != 0u);

    let line_compare = crtc(0x18u) | ((crtc(7u) & 0x10u) << 4u)
        | ((maximum_scan_line & 0x40u) << 3u);
    let preset_row_scan = crtc(8u);
    var pel_pan = attribute_reg(0x13u);
    if glyph_width == 9u {
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
        || (attribute_mode & 1u) != 0u || (graphics(6u) & 1u) != 0u;
    var colors: array<vec4<u32>, 4>;
    for (var color = 0u; color < 16u; color++) {
        colors[color / 4u][color % 4u] = attribute_color(color);
    }

    var output: TextVertex;
    output.position = vec4(positions[index], 0.0, 1.0);
    output.grid = vec4(crtc(1u) + 1u, glyph_width, (maximum_scan_line & 31u) + 1u, count_divisor);
    output.addressing = vec4((crtc(12u) << 8u) | crtc(13u), crtc(0x13u) * 2u, address_mode, underline);
    output.panning = vec4(preset_row_scan & 31u, (preset_row_scan >> 5u) & 3u, pel_pan, line_compare);
    output.fonts = vec3(font_a, font_b, u32(extended_memory && font_a != font_b));
    output.cursor = vec3((crtc(14u) << 8u) | crtc(15u), crtc(10u), crtc(11u));
    output.scanlines = vec2(1u << (maximum_scan_line >> 7u), state.scanline_step);
    output.display = vec3(attribute_mode, state.blink_on, u32(blanked));
    output.colors0 = colors[0];
    output.colors1 = colors[1];
    output.colors2 = colors[2];
    output.colors3 = colors[3];
    return output;
}

@fragment
fn fragment(input: TextVertex) -> @location(0) vec4<f32> {
    let blanked = input.display.z != 0u;
    if blanked {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }

    // Convert an output row into the CRTC row scan, including the split screen.
    let glyph_width = input.grid.y;
    let glyph_height = input.grid.z;
    let scan_repeat = input.scanlines.x;
    let attribute_mode = input.display.x;
    let blink_on = input.display.y != 0u;
    let line_compare = input.panning.w;
    let pixel = vec2<u32>(input.position.xy);
    let physical_row = pixel.y * input.scanlines.y;
    var row = physical_row / scan_repeat + input.panning.x;
    var start_address = input.addressing.x;
    var pel_pan = input.panning.z;
    if physical_row > line_compare {
        row = (physical_row - line_compare - 1u) / scan_repeat;
        start_address = 0u;
        if (attribute_mode & 0x20u) != 0u {
            pel_pan = 0u;
        }
    }

    // Character and attribute planes share the CRTC address.
    let underline = input.addressing.w;
    let count_divisor = input.grid.w;
    let glyph_row = row % glyph_height;
    let column = (pixel.x + pel_pan) / glyph_width;
    let dot = (pixel.x + pel_pan) % glyph_width;
    let line_start = start_address + (row / glyph_height) * input.addressing.y + input.panning.y;
    let counter = (line_start + column / count_divisor) & 0xffffu;
    let address = display_address(counter, glyph_row, input.addressing.z, underline);
    let character = vram_byte(0u, address);
    let cell_attribute = vram_byte(1u, address);

    // Glyphs live in plane 2. Underline and line-graphics rules alter their dots.
    let dual_font = input.fonts.z != 0u;
    let font = select(input.fonts.y, input.fonts.x, dual_font && (cell_attribute & 8u) != 0u);
    var glyph_bits = vram_byte(2u, font + character * 32u + glyph_row);
    if (cell_attribute & 0x77u) == 1u && glyph_row == (underline & 31u) {
        glyph_bits = 255u;
    }
    var foreground_pixel = dot < 8u && (glyph_bits & (0x80u >> dot)) != 0u;
    if dot == 8u && (attribute_mode & 4u) != 0u && character >= 0xc0u && character <= 0xdfu {
        foreground_pixel = (glyph_bits & 1u) != 0u;
    }

    // Apply character blink, then overlay the cursor during the visible blink phase.
    let blinking = (attribute_mode & 8u) != 0u;
    if blinking && (cell_attribute & 0x80u) != 0u && !blink_on {
        foreground_pixel = false;
    }
    let cursor_start = input.cursor.y;
    let cursor_end = input.cursor.z;
    let skew = (cursor_end >> 5u) & 3u;
    if blink_on && (cursor_start & 0x20u) == 0u
        && column >= skew && column < input.grid.x
        && ((line_start + (column - skew) / count_divisor) & 0xffffu) == input.cursor.x
        && glyph_row >= (cursor_start & 31u) && glyph_row <= (cursor_end & 31u) {
        foreground_pixel = true;
    }

    let foreground = cell_attribute & select(15u, 7u, dual_font);
    let background = (cell_attribute >> 4u) & select(15u, 7u, blinking);
    let color = select(background, foreground, foreground_pixel);
    let colors = array(input.colors0, input.colors1, input.colors2, input.colors3);
    return unpack4x8unorm(colors[color / 4u][color % 4u]);
}
