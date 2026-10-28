//! Decoded raster counts and exact conversion between time and character clocks.
//! Register encodings are interpreted by the register owners.

use crate::registers::Registers;
use std::time::Duration;

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Horizontal values count character clocks; vertical values count increments
/// of the vertical counter. `scanlines_per_vertical_count` relates that counter
/// to physical scanlines. Character height does not multiply the totals.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Timing {
    /// Zero while reset holds the counters or the selected input is disconnected.
    oscillator_hz: u64,
    oscillator_ticks_per_character: u64,
    pub horizontal_total: u64,
    pub horizontal_display: u64,
    pub horizontal_display_skew: u64,
    pub vertical_total: u64,
    pub vertical_display: u64,
    pub scanlines_per_vertical_count: u64,
    vertical_retrace_start: u64,
    vertical_retrace_end_low_bits: u64,
    pub reset_active: bool,
    sync_enabled: bool,
}

impl Timing {
    pub fn new(registers: &Registers) -> Self {
        let crtc = &registers.crtc;
        let sequencer = &registers.sequencer;
        let reset_active = sequencer.reset_active();
        Self {
            oscillator_hz: if reset_active {
                0
            } else {
                registers.oscillator_hz()
            },
            oscillator_ticks_per_character: sequencer.dots_per_character()
                * sequencer.dot_clock_divisor(),
            horizontal_total: crtc.horizontal_total(),
            horizontal_display: crtc.horizontal_display(),
            horizontal_display_skew: crtc.horizontal_display_skew(),
            vertical_total: crtc.vertical_total(),
            vertical_display: crtc.vertical_display(),
            scanlines_per_vertical_count: crtc.scanlines_per_vertical_count(),
            vertical_retrace_start: crtc.vertical_retrace_start(),
            vertical_retrace_end_low_bits: crtc.vertical_retrace_end_low_bits(),
            reset_active,
            sync_enabled: crtc.sync_enabled(),
        }
    }

    pub fn clock_running(self) -> bool {
        self.oscillator_hz != 0
    }

    pub fn frame_characters(self) -> u64 {
        self.horizontal_total * self.vertical_total * self.scanlines_per_vertical_count
    }

    // Fractional character position is numerator / phase_denominator(). An
    // elapsed nanosecond adds oscillator_hz to that numerator. Keeping the
    // division remainder avoids rounding every character period to nanoseconds.
    fn phase_denominator(self) -> u64 {
        NANOS_PER_SECOND * self.oscillator_ticks_per_character
    }

    /// Returns whole character clocks and carries the fractional position forward.
    pub fn elapsed_characters(self, elapsed: Duration, remainder: &mut u64) -> u128 {
        let numerator =
            elapsed.as_nanos() * u128::from(self.oscillator_hz) + u128::from(*remainder);
        let denominator = u128::from(self.phase_denominator());
        *remainder = (numerator % denominator) as u64;
        numerator / denominator
    }

    /// Preserves fractional character position when dots or clock division change.
    pub fn rescale_remainder(self, remainder: u64, old: Self) -> u64 {
        (u128::from(remainder) * u128::from(self.phase_denominator())
            / u128::from(old.phase_denominator())) as u64
    }

    pub fn vertical_retrace_active(self, vertical_count: u64) -> bool {
        let start = self.vertical_retrace_start;
        if self.reset_active || !self.sync_enabled || start >= self.vertical_total {
            return false;
        }
        // CRTC compares only the low four counter bits at retrace end. Find the
        // first match after the start; if the counter resets first, comparison
        // resumes from zero (IBM VGA Function p2-70).
        let end_low_bits = self.vertical_retrace_end_low_bits;
        let first_end_match =
            start + ((end_low_bits.wrapping_sub(start).wrapping_sub(1) & 0x0f) + 1);
        if vertical_count >= start {
            vertical_count < first_end_match
        } else {
            first_end_match >= self.vertical_total && vertical_count < end_low_bits
        }
    }
}
