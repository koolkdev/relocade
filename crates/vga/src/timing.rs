//! Decoded raster counts and exact conversion between time and dot clocks.
//! Register encodings are interpreted by the register owners.

use crate::registers::Registers;
use std::time::Duration;

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Horizontal values count dot clocks; vertical values count increments
/// of the vertical counter. `scanlines_per_vertical_count` relates that counter
/// to physical scanlines. Character height does not multiply the totals.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Timing {
    /// Zero while reset holds the counters or the selected input is disconnected.
    oscillator_hz: u64,
    dot_clock_divisor: u64,
    pub dots_per_character: u64,
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
            dot_clock_divisor: sequencer.dot_clock_divisor(),
            dots_per_character: sequencer.dots_per_character(),
            horizontal_total: crtc.horizontal_total() * sequencer.dots_per_character(),
            horizontal_display: crtc.horizontal_display() * sequencer.dots_per_character(),
            horizontal_display_skew: crtc.horizontal_display_skew()
                * sequencer.dots_per_character(),
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

    pub fn frame_dots(self) -> u64 {
        self.horizontal_total * self.vertical_total * self.scanlines_per_vertical_count
    }

    /// Internal retrace boundary used to sample display inputs. Disabling the
    /// external sync signals does not stop the counters or sampling boundaries.
    pub fn retrace_dot(self) -> Option<u64> {
        (self.clock_running() && self.vertical_retrace_start < self.vertical_total).then_some(
            self.vertical_retrace_start * self.scanlines_per_vertical_count * self.horizontal_total,
        )
    }

    // Fractional dot position is numerator / phase_denominator(). An
    // elapsed nanosecond adds oscillator_hz to that numerator. Keeping the
    // division remainder avoids rounding every dot period to nanoseconds.
    fn phase_denominator(self) -> u64 {
        NANOS_PER_SECOND * self.dot_clock_divisor
    }

    /// Returns whole dot clocks and carries the fractional position forward.
    pub fn elapsed_dots(self, elapsed: Duration, remainder: &mut u64) -> u128 {
        let numerator =
            elapsed.as_nanos() * u128::from(self.oscillator_hz) + u128::from(*remainder);
        let denominator = u128::from(self.phase_denominator());
        *remainder = (numerator % denominator) as u64;
        numerator / denominator
    }

    /// Preserves fractional dot position when clock division changes.
    pub fn rescale_remainder(self, remainder: u64, old: Self) -> u64 {
        (u128::from(remainder) * u128::from(self.phase_denominator())
            / u128::from(old.phase_denominator())) as u64
    }

    /// Time represented by whole dot clocks plus fractional progress.
    /// Round elapsed time down when dating an already completed boundary.
    pub fn elapsed_for_dots(self, dots: u64, remainder: u64) -> Duration {
        let numerator =
            u128::from(dots) * u128::from(self.phase_denominator()) + u128::from(remainder);
        Duration::from_nanos((numerator / u128::from(self.oscillator_hz)) as u64)
    }

    /// Time to a future dot boundary, accounting for current progress.
    /// Round up so the deadline cannot precede the boundary.
    pub fn remaining_for_dots(self, dots: u64, remainder: u64) -> Duration {
        let numerator =
            u128::from(dots) * u128::from(self.phase_denominator()) - u128::from(remainder);
        Duration::from_nanos(numerator.div_ceil(u128::from(self.oscillator_hz)) as u64)
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
