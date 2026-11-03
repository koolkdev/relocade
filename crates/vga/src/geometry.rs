//! Output geometry needed to allocate an image from captured display state.
mod sampling;

use crate::DisplaySnapshot;
use sampling::Sampling;
use std::ops::Range;

/// Image dimensions and the relationship between output rows and VGA scanlines.
/// Pixel interpretation belongs to the renderer and uses the captured registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayGeometry {
    /// The nonblanked raster, including the overscan border. A completely blank
    /// raster uses a one-pixel black image.
    pub size: [u32; 2],
    /// First image sample in raster dot clocks and physical scanlines. The image
    /// continues across the horizontal and vertical counter resets when needed.
    pub raster_origin: [u32; 2],
    /// Active picture dimensions at the image's sampling rate, before cropping
    /// or adding borders. These determine pixel proportions, not image size.
    pub active_size: [u32; 2],
    /// Physical scanlines represented by each image row. Identical repeated
    /// rows share storage unless panning, a split or blanking separates them.
    pub scanline_step: u32,
}

impl DisplaySnapshot {
    /// Output geometry for text, four-bit and eight-bit graphics layouts.
    pub fn geometry(&self) -> DisplayGeometry {
        let crtc = &self.registers.crtc;
        let dots_per_character = self.registers.sequencer.dots_per_character() as u32;
        let scanlines_per_count = crtc.scanlines_per_vertical_count() as u32;
        let horizontal = nonblank_span(
            crtc.horizontal_total() as u32,
            crtc.horizontal_blanking(),
            dots_per_character,
        );
        let vertical = nonblank_span(
            crtc.vertical_total() as u32,
            crtc.vertical_blanking(),
            scanlines_per_count,
        );
        let sampling = Sampling::new(&self.registers, &vertical);
        let active_width = crtc.horizontal_display() as u32 * dots_per_character;
        let active_height = crtc.vertical_display() as u32 * scanlines_per_count;

        let mut geometry = DisplayGeometry {
            size: [
                (horizontal.len() as u32).div_ceil(sampling.dots_per_pixel),
                vertical.len() as u32 / sampling.scanlines_per_row,
            ],
            raster_origin: [horizontal.start, vertical.start],
            active_size: [
                active_width.div_ceil(sampling.dots_per_pixel),
                active_height / sampling.scanlines_per_row,
            ],
            scanline_step: sampling.scanlines_per_row,
        };
        // GPU images need nonzero dimensions even when no raster samples survive.
        if horizontal.is_empty() || vertical.is_empty() {
            geometry.size = [1, 1];
            geometry.raster_origin = [0, 0];
        }
        geometry
    }
}

impl DisplayGeometry {
    /// Preserve the active picture's nominal 4:3 proportions when adding a
    /// border. Physical monitor size and position adjustments are not modeled.
    pub fn pixel_aspect_ratio(&self) -> (u32, u32) {
        let (width, height) = (self.active_size[1] * 4, self.active_size[0] * 3);
        let (mut a, mut b) = (width, height);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        (width / a, height / a)
    }
}

// Unwrap the nonblank interval from the end of blanking. Its end can exceed the
// total. Convert counters to raster dots/scanlines before choosing image samples;
// the shader wraps those coordinates at the corresponding raster total.
fn nonblank_span(total: u32, blanking: [Range<u32>; 2], units_per_count: u32) -> Range<u32> {
    let start = (blanking[0].end + blanking[1].end) % total;
    let length = total - blanking.iter().map(|range| range.len() as u32).sum::<u32>();
    start * units_per_count..(start + length) * units_per_count
}
