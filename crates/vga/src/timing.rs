//! Decoded raster counts and exact conversion between time and dot clocks.
//! Cards supply normalized counters for VGA or packed display output.
mod vga;

use std::time::Duration;

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Horizontal values count dot clocks; vertical values count increments
/// of the vertical counter. `scanlines_per_vertical_count` relates that counter
/// to physical scanlines.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Timing {
    /// Zero while reset holds the counters or the selected input is disconnected.
    oscillator_hz: u64,
    dot_clock_divisor: u64,
    /// Physical dots per increment of the horizontal counter.
    pub dots_per_horizontal_count: u64,
    pub horizontal_total: u64,
    pub horizontal_display: u64,
    pub horizontal_display_skew: u64,
    pub vertical_total: u64,
    pub vertical_display: u64,
    pub scanlines_per_vertical_count: u64,
    // Interlaced fields consume half the progressive vertical period.
    vertical_count_divisor: u64,
    vertical_retrace_start: u64,
    vertical_retrace_end: u64,
    /// End of a pulse continuing from the preceding frame, or zero.
    wrapped_retrace_end: u64,
    pub reset_active: bool,
    sync_enabled: bool,
}

impl Timing {
    pub(crate) fn packed(
        clock: u64,
        size: [u32; 2],
        total: [u32; 2],
        retrace: std::ops::Range<u64>,
    ) -> Self {
        Self {
            oscillator_hz: clock,
            dot_clock_divisor: 1,
            dots_per_horizontal_count: 1,
            horizontal_total: u64::from(total[0]),
            horizontal_display: u64::from(size[0]),
            horizontal_display_skew: 0,
            vertical_total: u64::from(total[1]),
            vertical_display: u64::from(size[1]),
            scanlines_per_vertical_count: 1,
            vertical_count_divisor: 1,
            vertical_retrace_start: retrace.start,
            vertical_retrace_end: retrace.end,
            wrapped_retrace_end: 0,
            reset_active: false,
            sync_enabled: true,
        }
    }

    /// Extend VGA's vertical counters without duplicating pulse-wrap handling.
    pub(crate) fn with_vertical_counts(mut self, total: u64, display: u64, retrace: u64) -> Self {
        let end_low = self.vertical_retrace_end & 15;
        self.vertical_total = total;
        self.vertical_display = display;
        self.vertical_retrace_start = retrace;
        self.vertical_retrace_end =
            retrace + ((end_low.wrapping_sub(retrace).wrapping_sub(1) & 15) + 1);
        self.wrapped_retrace_end = if retrace < total && self.vertical_retrace_end >= total {
            end_low
        } else {
            0
        };
        self
    }

    pub(crate) fn divide_clock(mut self, divisor: u64) -> Self {
        self.dot_clock_divisor *= divisor;
        self
    }

    pub(crate) fn with_interlace(mut self, enabled: bool) -> Self {
        self.vertical_count_divisor = if enabled { 2 } else { 1 };
        self
    }

    pub(crate) fn vertical_count_dots(self) -> u64 {
        self.horizontal_total * self.scanlines_per_vertical_count / self.vertical_count_divisor
    }

    pub fn clock_running(self) -> bool {
        self.oscillator_hz != 0
    }

    pub fn frame_dots(self) -> u64 {
        self.vertical_total * self.vertical_count_dots()
    }

    /// Internal retrace boundary used to sample display inputs. Disabling the
    /// external sync signals does not stop the counters or sampling boundaries.
    pub fn retrace_dot(self) -> Option<u64> {
        (self.clock_running() && self.vertical_retrace_start < self.vertical_total)
            .then_some(self.vertical_retrace_start * self.vertical_count_dots())
    }

    /// The observable status pulse can be suppressed while internal capture continues.
    pub fn retrace_pulse_dot(self) -> Option<u64> {
        if !self.vertical_retrace_active(self.vertical_retrace_start) {
            return None;
        }
        self.retrace_dot()
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
        (vertical_count >= start && vertical_count < self.vertical_retrace_end)
            || vertical_count < self.wrapped_retrace_end
    }

    pub fn input_status_1(self, dot: u64, horizontal: u64) -> u8 {
        let vertical = dot / self.vertical_count_dots();
        // Bit zero is inverted display enable, independent of programmed blanking.
        let disabled = self.reset_active
            || horizontal < self.horizontal_display_skew
            || horizontal >= self.horizontal_display_skew + self.horizontal_display
            || vertical >= self.vertical_display;
        u8::from(disabled) | (u8::from(self.vertical_retrace_active(vertical)) << 3)
    }

    /// Distance to the next possible status edge. While vertically outside the
    /// display, horizontal edges cannot change display enable and are skipped.
    /// Candidates at or beyond the programmed total do not occur in that frame.
    pub fn next_status_in_dots(self, dot: u64, horizontal: u64) -> Option<u64> {
        if !self.clock_running() {
            return None;
        }
        let total = self.frame_dots();
        let vertical_dots = self.vertical_count_dots();
        let vertical = dot / vertical_dots;
        // Wrapping can change either signal, including a pulse spanning frames.
        let mut next = total - dot;
        for boundary in [
            self.vertical_display,
            self.vertical_retrace_start,
            self.vertical_retrace_end,
            self.wrapped_retrace_end,
        ] {
            if boundary < self.vertical_total {
                let boundary = boundary * vertical_dots;
                if boundary > dot {
                    next = next.min(boundary - dot);
                }
            }
        }
        let start = self.horizontal_display_skew;
        let end = (start + self.horizontal_display).min(self.horizontal_total);
        if vertical < self.vertical_display && start < end {
            if horizontal < start {
                next = next.min(start - horizontal);
            } else if horizontal < end {
                if end < self.horizontal_total || start != 0 {
                    next = next.min(end - horizontal);
                }
            } else {
                next = next.min(self.horizontal_total - horizontal + start);
            }
        }
        Some(next)
    }
}
