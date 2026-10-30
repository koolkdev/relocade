// Character/attribute interpretation, font sampling and the text cursor.
fn text_color(input: DisplayVertex, line: Scanline, x: u32) -> u32 {
    let glyph_width = input.grid.y;
    let attribute_mode = input.display.x;
    let blink_on = input.display.y != 0u;
    // Character and attribute planes share the CRTC address.
    let underline = input.addressing.w;
    let count_divisor = input.grid.w;
    let glyph_row = line.row_scan;
    let column = (x + line.pel_pan) / glyph_width;
    let dot = (x + line.pel_pan) % glyph_width;
    let line_start = line.counter;
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
    return palette_color(input, color);
}
