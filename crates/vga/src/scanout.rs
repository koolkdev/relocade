//! Pixel generation from programmed display addresses, font RAM and colors.
//! Raster timing and app presentation are independent of this synchronous work.

mod text;

use crate::{
    memory::VideoMemory, raster::Boundary, registers::Registers, timing::Timing, VideoFrame,
};
use text::Text;

pub(super) fn render(
    memory: &VideoMemory,
    registers: &Registers,
    boundary: &Boundary,
) -> VideoFrame {
    let timing = Timing::new(registers);
    let width = timing
        .horizontal_display
        .min(timing.horizontal_total - timing.horizontal_display_skew) as u32;
    let height = (timing.vertical_display.min(timing.vertical_total)
        * timing.scanlines_per_vertical_count) as u32;
    let mut pixels = vec![0; width as usize * height as usize * 4].into_boxed_slice();
    let text = Text::new(memory, registers, boundary.blink_on);
    let crtc = &registers.crtc;
    let split_line = (crtc.line_compare() + 1) * timing.scanlines_per_vertical_count as u32;
    let blanked = registers.sequencer.screen_off() || !registers.attribute.output_enabled();
    for y in 0..height {
        let split = y >= split_line;
        let (line, start, preset) = if split {
            (y - split_line, 0, 0)
        } else {
            (y, crtc.start_address(), u32::from(crtc.preset_row_scan()))
        };
        let scan = line / crtc.scanline_repetitions() + preset;
        let row = scan / u32::from(crtc.character_height());
        let row_scan = (scan % u32::from(crtc.character_height())) as u8;
        let address = start
            .wrapping_add((row as u16).wrapping_mul(crtc.row_stride()))
            .wrapping_add(crtc.byte_panning());
        let pan = u32::from(
            registers
                .attribute
                .panning(timing.dots_per_character == 9, split),
        );
        for x in 0..width {
            let rgba = if blanked {
                [0, 0, 0, 255]
            } else {
                let dot = x + pan;
                let column = dot / timing.dots_per_character as u32;
                let color = text.pixel(
                    address,
                    column as u16,
                    row_scan,
                    (dot % timing.dots_per_character as u32) as u8,
                );
                registers.dac.rgba(registers.attribute.palette_index(color))
            };
            let offset = (y as usize * width as usize + x as usize) * 4;
            pixels[offset..offset + 4].copy_from_slice(&rgba);
        }
    }
    // VGA modes target a 4:3 monitor. Keep the physical active raster, including
    // repeated scanlines, and describe its pixel shape to the presenter.
    let (mut a, mut b) = (4 * height, 3 * width);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    VideoFrame {
        width,
        height,
        pixels,
        pixel_aspect_ratio: (4 * height / a, 3 * width / a),
        time: boundary.time,
        sequence: boundary.sequence,
    }
}
