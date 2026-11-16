//! Decoded raster counts and exact conversion between time and dot clocks.
//! Cards supply normalized counters for VGA or packed display output.
mod vga;

use relocade_machine::TickRate;

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

    fn phase_denominator(self, rate: TickRate) -> u128 {
        u128::from(rate.ticks_per_second().get()) * u128::from(self.dot_clock_divisor)
    }

    /// Converts native progress directly into dot clocks and retains the remainder.
    pub fn elapsed_dots(self, ticks: u64, rate: TickRate, remainder: &mut u128) -> u128 {
        let numerator = u128::from(ticks) * u128::from(self.oscillator_hz) + *remainder;
        let denominator = self.phase_denominator(rate);
        *remainder = numerator % denominator;
        numerator / denominator
    }

    /// Preserve the fractional dot when changing the oscillator divider.
    pub fn rescale_remainder(self, remainder: u128, old: Self) -> u128 {
        remainder * u128::from(self.dot_clock_divisor) / u128::from(old.dot_clock_divisor)
    }

    /// Native ticks since an already crossed boundary, rounded down.
    pub fn elapsed_for_dots(self, dots: u64, rate: TickRate, remainder: u128) -> u64 {
        let numerator = u128::from(dots) * self.phase_denominator(rate) + remainder;
        (numerator / u128::from(self.oscillator_hz))
            .try_into()
            .expect("past raster edge fits in native ticks")
    }

    /// Native ticks to a future dot boundary, rounded up without rounding a dot period.
    pub fn remaining_for_dots(self, dots: u64, rate: TickRate, remainder: u128) -> Option<u64> {
        let numerator = u128::from(dots).checked_mul(self.phase_denominator(rate))? - remainder;
        let ticks = numerator.div_ceil(u128::from(self.oscillator_hz));
        (ticks <= i64::MAX as u128).then_some(ticks as u64)
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
