// Included only by test crates and worker fixtures. Individual consumers use
// different portions of this reference and its image metadata.
#![allow(dead_code)]

use relocade_vga::{CapturedFrame, TextScreen};
use std::time::Duration;

#[path = "../../src/text.rs"]
mod text;
use text::*;

/// Convert an immutable text capture to pixels on the calling worker.
/// This function never reads or borrows the VGA device.
pub(crate) fn render(screen: &TextScreen, blink_on: bool, blanked: bool) -> Box<[u8]> {
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

/// Owned software-rendered image. Pixels are tightly packed, top-to-bottom RGBA8,
/// with sRGB color values and opaque alpha. Retaining an image requires no VGA owner.
#[derive(Debug)]
pub(crate) struct SoftwareFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Box<[u8]>,
    /// Pixel width / height. Combine with the image dimensions when sizing a viewport.
    pub pixel_aspect_ratio: (u32, u32),
    /// Elapsed session time at the completed frame boundary.
    pub time: Duration,
    /// Increasing frame identity; skipped frames can leave gaps.
    pub sequence: u64,
}

/// Test reference for captured text; never reads live device state.
pub(crate) fn rasterize(capture: &CapturedFrame) -> SoftwareFrame {
    SoftwareFrame {
        width: WIDTH,
        height: HEIGHT,
        pixels: render(&capture.screen, capture.blink_on, capture.blanked),
        pixel_aspect_ratio: PIXEL_ASPECT_RATIO,
        time: capture.time,
        sequence: capture.sequence,
    }
}
