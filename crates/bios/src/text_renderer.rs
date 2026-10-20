use crate::TextScreen;
use std::time::Duration;

const GLYPH_WIDTH: usize = 8;
const GLYPH_HEIGHT: usize = 8;
const FONT: &[u8; 256 * GLYPH_HEIGHT] = include_bytes!("../assets/modern-dos-8x8.bin");
const PALETTE: [[u8; 4]; 16] = [
    [0x00, 0x00, 0x00, 0xff],
    [0x00, 0x00, 0xaa, 0xff],
    [0x00, 0xaa, 0x00, 0xff],
    [0x00, 0xaa, 0xaa, 0xff],
    [0xaa, 0x00, 0x00, 0xff],
    [0xaa, 0x00, 0xaa, 0xff],
    [0xaa, 0x55, 0x00, 0xff],
    [0xaa, 0xaa, 0xaa, 0xff],
    [0x55, 0x55, 0x55, 0xff],
    [0x55, 0x55, 0xff, 0xff],
    [0x55, 0xff, 0x55, 0xff],
    [0x55, 0xff, 0xff, 0xff],
    [0xff, 0x55, 0x55, 0xff],
    [0xff, 0x55, 0xff, 0xff],
    [0xff, 0xff, 0x55, 0xff],
    [0xff, 0xff, 0xff, 0xff],
];

/// Rasterizes the initial 80-by-25 text mode with a fixed 8-by-8 CP437 font.
/// The owned pixel buffer is reused on every call. Presentation and GPU access
/// belong to the caller; rendering does not access or modify guest memory.
pub struct TextRenderer {
    pixels: Box<[u8]>,
}

impl TextRenderer {
    pub const WIDTH: u32 = (TextScreen::COLUMNS * GLYPH_WIDTH) as u32;
    pub const HEIGHT: u32 = (TextScreen::ROWS * GLYPH_HEIGHT) as u32;

    pub fn new() -> Self {
        Self {
            pixels: vec![0; Self::WIDTH as usize * Self::HEIGHT as usize * 4].into_boxed_slice(),
        }
    }

    /// Returns tightly packed, top-to-bottom RGBA8 pixels with opaque alpha and
    /// sRGB color values. `elapsed` is time since presentation started, independent
    /// of the guest clock. Cursor and attribute blinking alternate every 500 ms.
    ///
    /// Attribute bit 7 enables character blinking; background colors use bits 4–6.
    /// Cursor scan lines use the low five bits, and start bit 5 hides the cursor.
    /// Reversed shapes and positions outside the screen are hidden.
    ///
    /// ```
    /// # use relocade_bios::{TextRenderer, TextScreen};
    /// # use std::time::Duration;
    /// # fn draw(screen: &TextScreen, elapsed: Duration) {
    /// let mut renderer = TextRenderer::new();
    /// let rgba = renderer.render(screen, elapsed);
    /// assert_eq!(rgba.len(), (TextRenderer::WIDTH * TextRenderer::HEIGHT * 4) as usize);
    /// # }
    /// ```
    pub fn render(&mut self, screen: &TextScreen, elapsed: Duration) -> &[u8] {
        let blink_on = (elapsed.as_millis() / 500).is_multiple_of(2);
        let cursor = screen.cursor;
        let cursor_visible = blink_on && cursor.start_scan_line & 0x20 == 0;
        let cursor_start = usize::from(cursor.start_scan_line & 0x1f);
        let cursor_end = usize::from(cursor.end_scan_line & 0x1f);
        for (index, cell) in screen.cells.iter().enumerate() {
            let row = index / TextScreen::COLUMNS;
            let column = index % TextScreen::COLUMNS;
            let foreground = PALETTE[usize::from(cell.attribute & 0x0f)];
            let background = PALETTE[usize::from((cell.attribute >> 4) & 7)];
            let glyph_visible = blink_on || cell.attribute & 0x80 == 0;
            let cursor_here = cursor_visible
                && row == usize::from(cursor.row)
                && column == usize::from(cursor.column);
            let glyph_start = usize::from(cell.character) * GLYPH_HEIGHT;
            for (y, &bits) in FONT[glyph_start..glyph_start + GLYPH_HEIGHT]
                .iter()
                .enumerate()
            {
                let cursor_line = cursor_here && (cursor_start..=cursor_end).contains(&y);
                let offset =
                    ((row * GLYPH_HEIGHT + y) * Self::WIDTH as usize + column * GLYPH_WIDTH) * 4;
                for (x, pixel) in self.pixels[offset..offset + GLYPH_WIDTH * 4]
                    .chunks_exact_mut(4)
                    .enumerate()
                {
                    let lit = cursor_line || (glyph_visible && bits & (0x80 >> x) != 0);
                    pixel.copy_from_slice(if lit { &foreground } else { &background });
                }
            }
        }
        &self.pixels
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}
