const COLUMNS: u32 = 80;
const ROWS: u32 = 25;
const GLYPH_WIDTH: u32 = 8;
const GLYPH_HEIGHT: u32 = 8;

struct Text {
    start_address: u32,
    cursor_address: u32,
    cursor_start: u32,
    cursor_end: u32,
    blink_on: u32,
    blanked: u32,
    reserved: vec2<u32>,
    // Eight character/attribute pairs per vector, in display order.
    cells: array<vec4<u32>, 250>,
}

@group(0) @binding(0) var<uniform> text: Text;
@group(0) @binding(1) var font: texture_2d<u32>;
@group(0) @binding(2) var palette: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}

fn cursor_covers(cell: u32, glyph_row: u32) -> bool {
    let start = text.cursor_start;
    let end = text.cursor_end;
    let cursor_index = (text.cursor_address - text.start_address) & 0xffffu;
    let skew = (end >> 5u) & 3u;
    return text.blink_on != 0u && (start & 0x20u) == 0u
        && cursor_index < COLUMNS * ROWS
        && cursor_index % COLUMNS + skew < COLUMNS
        && cell == cursor_index + skew
        && glyph_row >= (start & 0x1fu) && glyph_row <= (end & 0x1fu);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    if text.blanked != 0u {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    let pixel = vec2<u32>(position.xy);
    let cell = (pixel.y / GLYPH_HEIGHT) * COLUMNS + pixel.x / GLYPH_WIDTH;
    let word = text.cells[cell / 8u][(cell / 2u) % 4u];
    let pair = (word >> ((cell % 2u) * 16u)) & 0xffffu;
    let character = pair & 0xffu;
    let attributes = pair >> 8u;
    let glyph_row = pixel.y % GLYPH_HEIGHT;
    let glyph_bits = textureLoad(font, vec2<i32>(i32(glyph_row), i32(character)), 0).x;
    let glyph_visible = text.blink_on != 0u || (attributes & 0x80u) == 0u;
    let glyph_lit = (glyph_bits & (0x80u >> (pixel.x % GLYPH_WIDTH))) != 0u;
    let foreground = attributes & 0x0fu;
    let background = (attributes >> 4u) & 7u;
    let lit = (glyph_visible && glyph_lit) || cursor_covers(cell, glyph_row);
    return textureLoad(palette, vec2<i32>(i32(select(background, foreground, lit)), 0), 0);
}
