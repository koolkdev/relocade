//! Fixed raster for the validated initial DISPI display layout.
use super::Timing;
use crate::SvgaLayout;

impl Timing {
    pub fn svga(layout: SvgaLayout) -> Self {
        // 25.175 MHz / (800 dots * 525 lines), about 59.94 Hz.
        Self {
            oscillator_hz: 25_175_000,
            dot_clock_divisor: 1,
            dots_per_horizontal_count: 1,
            horizontal_total: 800,
            horizontal_display: u64::from(layout.size()[0]),
            horizontal_display_skew: 0,
            vertical_total: 525,
            vertical_display: u64::from(layout.size()[1]),
            scanlines_per_vertical_count: 1,
            vertical_retrace_start: 490,
            vertical_retrace_end: 492,
            wrapped_retrace_end: 0,
            reset_active: false,
            sync_enabled: true,
        }
    }
}
