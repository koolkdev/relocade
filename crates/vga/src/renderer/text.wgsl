struct Text {
    columns: u32, glyph_width: u32,
    glyph_height: u32, start_address: u32, stride: u32, preset_row: u32,
    byte_panning: u32, pel_panning: u32, line_compare: u32, scan_repeat: u32,
    address_mode: u32, underline: u32, count_divisor: u32, font_a: u32,
    font_b: u32, dual_font: u32, cursor_address: u32, cursor_start: u32,
    cursor_end: u32, attribute_mode: u32, blink_on: u32, blanked: u32,
    output_step: u32, reserved: u32,
    colors: array<vec4<u32>, 4>,
}
@group(0) @binding(0) var<uniform> text: Text;
@group(0) @binding(1) var memory: texture_2d_array<u32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}

fn read_byte(plane: u32, address: u32) -> u32 {
    let packed = textureLoad(memory, vec2<i32>(i32((address & 255u) / 4u), i32((address & 0xffffu) / 256u)), i32(plane), 0);
    return packed[address & 3u];
}

fn display_address(counter: u32, row: u32) -> u32 {
    var address = counter;
    if (text.underline & 0x40u) != 0u {
        address = (counter << 2u) | ((counter >> 12u) & 3u);
    } else if (text.address_mode & 0x40u) == 0u {
        let wrap = select(13u, 15u, (text.address_mode & 0x20u) != 0u);
        address = (counter << 1u) | ((counter >> wrap) & 1u);
    }
    if (text.address_mode & 1u) == 0u { address = (address & ~0x2000u) | ((row & 1u) << 13u); }
    if (text.address_mode & 2u) == 0u { address = (address & ~0x4000u) | ((row & 2u) << 13u); }
    return address & 0xffffu;
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    if text.blanked != 0u { return vec4(0.0, 0.0, 0.0, 1.0); }
    let pixel = vec2<u32>(position.xy);
    let physical_row = pixel.y * text.output_step;
    let split = physical_row > text.line_compare;
    var row = physical_row / text.scan_repeat + text.preset_row;
    var start = text.start_address;
    var pan = text.pel_panning;
    if split {
        row = (physical_row - text.line_compare - 1u) / text.scan_repeat;
        start = 0u;
        if (text.attribute_mode & 0x20u) != 0u { pan = 0u; }
    }
    let glyph_row = row % text.glyph_height;
    let column = (pixel.x + pan) / text.glyph_width;
    let dot = (pixel.x + pan) % text.glyph_width;
    let line_start = start + (row / text.glyph_height) * text.stride + text.byte_panning;
    let counter = (line_start + column / text.count_divisor) & 0xffffu;
    let address = display_address(counter, glyph_row);
    let character = read_byte(0u, address);
    let cell_attribute = read_byte(1u, address);
    let font = select(text.font_b, text.font_a, text.dual_font != 0u && (cell_attribute & 8u) != 0u);
    var bits = read_byte(2u, font + character * 32u + glyph_row);
    if (cell_attribute & 0x77u) == 1u && glyph_row == (text.underline & 31u) { bits = 255u; }
    var lit = dot < 8u && (bits & (0x80u >> dot)) != 0u;
    if dot == 8u && (text.attribute_mode & 4u) != 0u && character >= 0xc0u && character <= 0xdfu {
        lit = (bits & 1u) != 0u;
    }
    let blinking = (text.attribute_mode & 8u) != 0u;
    if blinking && (cell_attribute & 0x80u) != 0u && text.blink_on == 0u { lit = false; }
    let skew = (text.cursor_end >> 5u) & 3u;
    if text.blink_on != 0u && (text.cursor_start & 0x20u) == 0u
        && column >= skew && column < text.columns
        && ((line_start + (column - skew) / text.count_divisor) & 0xffffu) == text.cursor_address
        && glyph_row >= (text.cursor_start & 31u) && glyph_row <= (text.cursor_end & 31u) { lit = true; }
    let foreground = cell_attribute & select(15u, 7u, text.dual_font != 0u);
    let background = (cell_attribute >> 4u) & select(15u, 7u, blinking);
    let color = select(background, foreground, lit);
    let rgba = text.colors[color / 4u][color % 4u];
    return vec4<f32>(f32(rgba & 255u), f32((rgba >> 8u) & 255u), f32((rgba >> 16u) & 255u), 255.0) / 255.0;
}
