//! Programmable VGA timing decoded before the common raster consumes it.
//! Character height does not multiply raster totals.
use super::Timing;
use crate::registers::Registers;

impl Timing {
    pub fn vga(registers: &Registers) -> Self {
        let crtc = &registers.crtc.registers;
        let sequencer = &registers.sequencer;
        let reset_active = sequencer.reset_active();
        let vertical_total = crtc.vertical_total();
        let retrace_start = crtc.vertical_retrace_start();
        let end_low_bits = crtc.vertical_retrace_end_low_bits();
        // CRTC compares only four low bits. Resolve the first match after start;
        // after a counter reset, comparison resumes from zero (IBM p2-70).
        let first_end =
            retrace_start + ((end_low_bits.wrapping_sub(retrace_start).wrapping_sub(1) & 0x0f) + 1);
        Self {
            oscillator_hz: if reset_active {
                0
            } else {
                registers.oscillator_hz()
            },
            dot_clock_divisor: sequencer.dot_clock_divisor(),
            dots_per_horizontal_count: sequencer.dots_per_character(),
            horizontal_total: crtc.horizontal_total() * sequencer.dots_per_character(),
            horizontal_display: crtc.horizontal_display() * sequencer.dots_per_character(),
            horizontal_display_skew: crtc.horizontal_display_skew()
                * sequencer.dots_per_character(),
            vertical_total,
            vertical_display: crtc.vertical_display(),
            scanlines_per_vertical_count: crtc.scanlines_per_vertical_count(),
            vertical_retrace_start: retrace_start,
            vertical_retrace_end: first_end,
            wrapped_retrace_end: if retrace_start < vertical_total && first_end >= vertical_total {
                end_low_bits
            } else {
                0
            },
            reset_active,
            sync_enabled: crtc.sync_enabled(),
        }
    }
}
