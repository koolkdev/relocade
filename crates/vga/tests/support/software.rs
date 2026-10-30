// Independent CPU rendering for tests and worker fixtures. All text settings
// come from raw captures; literal pixel cases check both this and the GPU shader.
#![allow(dead_code)]
use super::text;
use relocade_vga::{CapturedFrame, DisplaySnapshot};
use std::time::Duration;

pub(crate) fn render(snapshot: &DisplaySnapshot) -> Box<[u8]> {
    let text = text::Text::new(snapshot);
    let [width, height] = text.layout.size;
    let mut pixels = vec![0; width as usize * height as usize * 4].into_boxed_slice();
    for y in 0..height {
        let line = text.layout.scanline(y);
        for x in 0..width {
            let column = (x + line.pan) / text.layout.glyph_width;
            let dot = (x + line.pan) % text.layout.glyph_width;
            let counter = line
                .counter
                .wrapping_add((column / text.layout.count_divisor) as u16);
            let address = usize::from(text.layout.address(counter, line.glyph_row));
            let character = snapshot.plane(0)[address];
            let attribute = snapshot.plane(1)[address];
            let font = usize::from(text.font_address(character, attribute, line.glyph_row));
            let bits = if attribute & 0x77 == 1 && line.glyph_row == text.layout.underline & 31 {
                255
            } else {
                snapshot.plane(2)[font]
            };
            let mut lit = if dot < 8 {
                bits & (0x80 >> dot) != 0
            } else {
                text.attribute_mode & 4 != 0 && (0xc0..=0xdf).contains(&character) && bits & 1 != 0
            };
            let blinking = text.attribute_mode & 8 != 0;
            if blinking && attribute & 0x80 != 0 && !text.blink_on {
                lit = false;
            }
            let skew = u32::from((text.cursor_end >> 5) & 3);
            lit |= text.blink_on
                && text.cursor_start & 0x20 == 0
                && column >= skew
                && column < text.layout.columns
                && line
                    .counter
                    .wrapping_add(((column - skew) / text.layout.count_divisor) as u16)
                    == text.cursor_address
                && (text.cursor_start & 31..=text.cursor_end & 31).contains(&line.glyph_row);
            let color = if lit {
                attribute & if text.dual_font { 7 } else { 15 }
            } else {
                (attribute >> 4) & if blinking { 7 } else { 15 }
            };
            let rgba = if text.blanked {
                [0, 0, 0, 255]
            } else {
                text.colors[usize::from(color)].to_le_bytes()
            };
            let offset = ((y * width + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&rgba);
        }
    }
    pixels
}

#[derive(Debug)]
pub(crate) struct SoftwareFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Box<[u8]>,
    pub pixel_aspect_ratio: (u32, u32),
    pub time: Duration,
    pub sequence: u64,
}

pub(crate) fn rasterize(capture: &CapturedFrame) -> SoftwareFrame {
    let text = text::Text::new(&capture.display);
    SoftwareFrame {
        width: text.layout.size[0],
        height: text.layout.size[1],
        pixels: render(&capture.display),
        pixel_aspect_ratio: text.layout.pixel_aspect_ratio(),
        time: capture.time,
        sequence: capture.sequence,
    }
}
