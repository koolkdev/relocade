//! The plain DOS text profile: CP437, 80 columns, 60 lines per page.

use relocade_vga::registers::text_mode::FONT;

mod pdf;

const COLUMNS: usize = 80;
const ROWS: usize = 60;
const GLYPH_HEIGHT: usize = 8;
const WIDTH: usize = COLUMNS * 8;
const HEIGHT: usize = ROWS * GLYPH_HEIGHT;
const PAGE_BYTES: usize = COLUMNS * HEIGHT;
const MAX_PAGES: usize = 64;

/// Renders a plain DOS text snapshot to a US Letter PDF with half-inch margins.
///
/// CR returns to the left margin; LF advances a line without returning the head.
/// Tabs use eight-column stops. BS backs up one column and subsequent glyphs
/// overprint. FF ejects the current page, including an explicitly blank page;
/// a final FF adds no extra page. Text wraps at column 80 and line 60. NUL and
/// BEL are ignored. Other control commands, including ESC, are unsupported.
///
/// The bundled CP437 bitmap glyphs avoid platform font substitutions. At most
/// 64 pages are rendered, including blank pages, bounding form-feed expansion.
pub fn render_pdf(bytes: &[u8]) -> Result<Vec<u8>, String> {
    Ok(pdf::encode(layout(bytes)?))
}

fn layout(bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut text = Text {
        pages: Vec::new(),
        ink: vec![0; PAGE_BYTES],
        row: 0,
        column: 0,
        touched: false,
    };
    for &byte in bytes {
        match byte {
            0 | 7 => {}
            8 => text.column = text.column.saturating_sub(1),
            b'\t' => text.column = ((text.column / 8 + 1) * 8).min(COLUMNS),
            b'\r' => text.column = 0,
            b'\n' => {
                text.touched = true;
                text.next_line()?;
            }
            0x0c => {
                text.eject()?;
                text.column = 0;
            }
            1..=31 => {
                return Err(format!(
                    "Plain text printing does not support control byte 0x{byte:02X}"
                ))
            }
            _ => text.character(byte)?,
        }
    }
    if text.touched {
        text.eject()?;
    }
    if text.pages.is_empty() {
        return Err("There is no printer output to save".into());
    }
    Ok(text.pages)
}

struct Text {
    pages: Vec<Vec<u8>>,
    ink: Vec<u8>,
    row: usize,
    column: usize,
    touched: bool,
}

impl Text {
    fn character(&mut self, byte: u8) -> Result<(), String> {
        if self.column == COLUMNS {
            self.column = 0;
            self.next_line()?;
        }
        let glyph = usize::from(byte) * GLYPH_HEIGHT;
        for y in 0..GLYPH_HEIGHT {
            self.ink[(self.row * GLYPH_HEIGHT + y) * COLUMNS + self.column] |= FONT[glyph + y];
        }
        self.touched = true;
        self.column += 1;
        Ok(())
    }

    fn next_line(&mut self) -> Result<(), String> {
        self.row += 1;
        if self.row == ROWS {
            self.eject()?;
        }
        Ok(())
    }

    fn eject(&mut self) -> Result<(), String> {
        if self.pages.len() == MAX_PAGES {
            return Err(format!("Plain text printing is limited to {MAX_PAGES} pages; clear the capture to start a new job"));
        }
        let ink = std::mem::replace(&mut self.ink, vec![0; PAGE_BYTES]);
        self.pages.push(ink);
        self.row = 0;
        self.touched = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
