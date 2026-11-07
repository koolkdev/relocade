//! VESA DMT 60-Hz rasters for the supported packed modes.
//! https://glenwing.github.io/docs/VESA-DMT-1.13.pdf (DMT IDs 04h, 09h, 10h).
use super::Timing;
use crate::{SvgaLayout, SvgaResolution};

impl Timing {
    pub fn svga(layout: SvgaLayout) -> Self {
        let (oscillator_hz, horizontal_total, vertical_total, retrace) =
            match layout.mode().resolution() {
                SvgaResolution::R640x480 => (25_175_000, 800, 525, 490..492),
                SvgaResolution::R800x600 => (40_000_000, 1056, 628, 601..605),
                SvgaResolution::R1024x768 => (65_000_000, 1344, 806, 771..777),
            };
        Self {
            oscillator_hz,
            dot_clock_divisor: 1,
            dots_per_horizontal_count: 1,
            horizontal_total,
            horizontal_display: u64::from(layout.size()[0]),
            horizontal_display_skew: 0,
            vertical_total,
            vertical_display: u64::from(layout.size()[1]),
            scanlines_per_vertical_count: 1,
            vertical_retrace_start: retrace.start,
            vertical_retrace_end: retrace.end,
            wrapped_retrace_end: 0,
            reset_active: false,
            sync_enabled: true,
        }
    }
}
