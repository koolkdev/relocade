// Interpret character/attribute data using prepared font and cursor controls.
fn text_color(input: DisplayVertex, line: Scanline, x: u32) -> u32 {
    let glyph_row = line.row_scan;
    let column = (x + line.pel_pan) / state.glyph_width;
    let dot = (x + line.pel_pan) % state.glyph_width;
    let line_start = line.counter + line.byte_pan;
    let counter = (line_start + column / state.count_divisor) & state.plane_address_mask;
    let address = display_address(counter, glyph_row);
    let character = vga_byte(0u, address);
    let cell_attribute = vga_byte(1u, address);

    let font = select(state.font_b, state.font_a, (cell_attribute & state.font_select_mask) != 0u);
    var glyph_bits = vga_byte(2u, font + character * 32u + glyph_row);
    if (cell_attribute & 0x77u) == 1u && glyph_row == state.underline_row {
        glyph_bits = 255u;
    }
    var foreground_pixel = dot < 8u && (glyph_bits & (0x80u >> dot)) != 0u;
    if dot == 8u && state.line_graphics != 0u && character >= 0xc0u && character <= 0xdfu {
        foreground_pixel = (glyph_bits & 1u) != 0u;
    }

    if (cell_attribute & state.blink_mask) != 0u && state.attribute_blink_on == 0u {
        foreground_pixel = false;
    }
    if state.cursor_blink_on != 0u
        && column >= state.cursor_skew && column < state.columns
        && ((line_start + (column - state.cursor_skew) / state.count_divisor) & state.plane_address_mask) == state.cursor_address
        && glyph_row >= state.cursor_first_row && glyph_row < state.cursor_end_row {
        foreground_pixel = true;
    }

    let foreground = cell_attribute & state.foreground_mask;
    let background = (cell_attribute >> 4u) & state.background_mask;
    return palette_color(input, select(background, foreground, foreground_pixel));
}
