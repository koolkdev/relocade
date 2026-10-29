use crate::TextScreen;

const GLYPH_WIDTH: usize = 8;
const GLYPH_HEIGHT: usize = 8;
const FONT: &[u8; 256 * GLYPH_HEIGHT] = include_bytes!("../assets/modern-dos-8x8.bin");

// Fixed PC text colors, in text-attribute index order.
// Each entry is [red, green, blue, alpha]; 0xff alpha is fully opaque.
const PALETTE: [[u8; 4]; 16] = [
    [0x00, 0x00, 0x00, 0xff], // 0x0: Black
    [0x00, 0x00, 0xaa, 0xff], // 0x1: Blue
    [0x00, 0xaa, 0x00, 0xff], // 0x2: Green
    [0x00, 0xaa, 0xaa, 0xff], // 0x3: Cyan
    [0xaa, 0x00, 0x00, 0xff], // 0x4: Red
    [0xaa, 0x00, 0xaa, 0xff], // 0x5: Magenta
    [0xaa, 0x55, 0x00, 0xff], // 0x6: Brown
    [0xaa, 0xaa, 0xaa, 0xff], // 0x7: Light gray
    [0x55, 0x55, 0x55, 0xff], // 0x8: Dark gray
    [0x55, 0x55, 0xff, 0xff], // 0x9: Light blue
    [0x55, 0xff, 0x55, 0xff], // 0xa: Light green
    [0x55, 0xff, 0xff, 0xff], // 0xb: Light cyan
    [0xff, 0x55, 0x55, 0xff], // 0xc: Light red
    [0xff, 0x55, 0xff, 0xff], // 0xd: Light magenta
    [0xff, 0xff, 0x55, 0xff], // 0xe: Yellow
    [0xff, 0xff, 0xff, 0xff], // 0xf: White
];

pub(super) const WIDTH: u32 = (TextScreen::COLUMNS * GLYPH_WIDTH) as u32;
pub(super) const HEIGHT: u32 = (TextScreen::ROWS * GLYPH_HEIGHT) as u32;

/// Convert an immutable text capture to pixels on the calling worker.
/// This function never reads or borrows the VGA device.
pub(super) fn render(screen: &TextScreen, blink_on: bool, blanked: bool) -> Box<[u8]> {
    let mut pixels = vec![0; WIDTH as usize * HEIGHT as usize * 4].into_boxed_slice();
    if blanked {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            *pixel = [0, 0, 0, 255];
        }
        return pixels;
    }
    let cursor = screen.cursor;
    let cursor_index = usize::from(cursor.address.wrapping_sub(screen.start_address));
    let cursor_skew = usize::from((cursor.end_scan_line >> 5) & 3);
    let cursor_visible = blink_on
        && cursor.start_scan_line & 0x20 == 0
        && cursor_index < screen.cells.len()
        && cursor_index % TextScreen::COLUMNS + cursor_skew < TextScreen::COLUMNS;
    let cursor_start = usize::from(cursor.start_scan_line & 0x1f);
    let cursor_end = usize::from(cursor.end_scan_line & 0x1f);
    for (index, cell) in screen.cells.iter().enumerate() {
        let row = index / TextScreen::COLUMNS;
        let column = index % TextScreen::COLUMNS;
        let foreground = PALETTE[usize::from(cell.attribute & 0x0f)];
        let background = PALETTE[usize::from((cell.attribute >> 4) & 7)];
        let glyph_visible = blink_on || cell.attribute & 0x80 == 0;
        let cursor_here = cursor_visible && index == cursor_index + cursor_skew;
        let glyph_start = usize::from(cell.character) * GLYPH_HEIGHT;
        for (y, &bits) in FONT[glyph_start..glyph_start + GLYPH_HEIGHT]
            .iter()
            .enumerate()
        {
            let cursor_line = cursor_here && (cursor_start..=cursor_end).contains(&y);
            let offset = ((row * GLYPH_HEIGHT + y) * WIDTH as usize + column * GLYPH_WIDTH) * 4;
            for (x, pixel) in pixels[offset..offset + GLYPH_WIDTH * 4]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                let lit = cursor_line || (glyph_visible && bits & (0x80 >> x) != 0);
                pixel.copy_from_slice(if lit { &foreground } else { &background });
            }
        }
    }
    pixels
}

#[cfg(test)]
mod tests;
