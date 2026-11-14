// Prepared VGA settings occupy 208 bytes; offsets match renderer/state.rs.
struct VgaState {
    display_start: u32,
    blanked: u32,
    attribute_blink_on: u32,
    cursor_blink_on: u32,
    columns: u32,
    glyph_width: u32,
    row_height: u32,
    count_divisor: u32,
    stride: u32,
    scan_repeat: u32,
    scanline_step: u32,
    split_scanline: u32,
    preset_row: u32,
    byte_pan: u32,
    pel_pan: u32,
    reset_panning: u32,
    plane_address_mask: u32,
    address_shift: u32,
    wrap_shift: u32,
    wrap_mask: u32,
    row_scan_mask: u32,
    mode: u32,
    load_interval: u32,
    pixels_per_clock: u32,
    font_a: u32,
    font_b: u32,
    font_select_mask: u32,
    foreground_mask: u32,
    background_mask: u32,
    blink_mask: u32,
    line_graphics: u32,
    underline_row: u32,
    cursor_address: u32,
    cursor_first_row: u32,
    cursor_end_row: u32,
    cursor_skew: u32,
    palette: array<vec4<u32>, 2>,
    horizontal_blank: vec4<u32>,
    vertical_blank: vec4<u32>,
}

@group(0) @binding(0) var<uniform> state: VgaState;

// Decoder tags shared with scanout/parameters.rs.
const TEXT = 0u;
const PLANAR = 1u;
const INTERLEAVED = 2u;
const INDEXED256 = 3u;

fn vga_byte(plane: u32, address: u32) -> u32 {
    // Each word contains the four planes at one VGA plane offset.
    return packed_byte(memory[address & state.plane_address_mask], plane);
}
