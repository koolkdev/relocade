use crate::timing::Timing;
use std::time::Duration;

const DISPLAY_DISABLED: u8 = 1 << 0;
const VERTICAL_RETRACE: u8 = 1 << 3;
const BLINK_HALF_PERIOD_FRAMES: u8 = 16;
const BLINK_PERIOD_FRAMES: u8 = 2 * BLINK_HALF_PERIOD_FRAMES;

/// Raster position at `updated`. Register writes advance using the old timing
/// before reconfiguring, so programming cannot retime completed frames.
pub(super) struct Raster {
    timing: Timing,
    updated: Duration,
    dot: u64,
    /// Fractional dot position in the units maintained by `Timing`.
    dot_remainder: u64,
    pub retrace_count: u64,
}

impl Raster {
    pub fn new(now: Duration, timing: Timing) -> Self {
        Self {
            timing,
            updated: now,
            dot: 0,
            dot_remainder: 0,
            retrace_count: 0,
        }
    }

    /// Update counters to `now`, returning the latest crossed retrace time.
    /// Multiple missed retraces contribute to the count without a per-frame loop.
    pub fn update_to(&mut self, now: Duration) -> Option<Duration> {
        let timing = self.timing;
        let elapsed = now.saturating_sub(self.updated);
        self.updated = now;
        if !timing.clock_running() {
            return None;
        }
        let dots = timing.elapsed_dots(elapsed, &mut self.dot_remainder);
        let frame_dots = timing.frame_dots();
        let completed = timing.retrace_dot().and_then(|boundary| {
            let since_previous_boundary = (self.dot + frame_dots - boundary) % frame_dots;
            let advanced = u128::from(since_previous_boundary) + dots;
            let frames = advanced / u128::from(frame_dots);
            if frames == 0 {
                return None;
            }
            self.retrace_count = self
                .retrace_count
                .checked_add(u64::try_from(frames).expect("VGA frame count fits in u64"))
                .expect("VGA frame sequence exhausted");
            let since_latest_boundary = (advanced % u128::from(frame_dots)) as u64;
            Some(now - timing.elapsed_for_dots(since_latest_boundary, self.dot_remainder))
        });
        self.dot = ((u128::from(self.dot) + dots) % u128::from(frame_dots)) as u64;
        completed
    }

    /// Called after advancing elapsed time under the preceding configuration.
    /// Returns whether the activity must replace its current deadline.
    pub fn set_timing(&mut self, new: Timing) -> bool {
        let old = self.timing;
        if old == new {
            return false;
        }
        self.timing = new;
        if old.reset_active || new.reset_active {
            self.dot = 0;
            self.dot_remainder = 0;
        } else {
            let physical_line = self.dot / old.horizontal_total;
            let vertical_count = physical_line / old.scanlines_per_vertical_count;
            let subline = physical_line % old.scanlines_per_vertical_count;
            let line = vertical_count * new.scanlines_per_vertical_count
                + subline * new.scanlines_per_vertical_count / old.scanlines_per_vertical_count;
            let horizontal = self.dot % old.horizontal_total;
            let horizontal = horizontal / old.dots_per_character * new.dots_per_character
                + (horizontal % old.dots_per_character).min(new.dots_per_character - 1);
            self.dot = (line * new.horizontal_total + horizontal % new.horizontal_total)
                % new.frame_dots();
            self.dot_remainder = new.rescale_remainder(self.dot_remainder, old);
        }
        true
    }

    pub fn blink_on(&self) -> bool {
        self.retrace_count % u64::from(BLINK_PERIOD_FRAMES) < u64::from(BLINK_HALF_PERIOD_FRAMES)
    }

    pub fn next_retrace_at(&self) -> Option<Duration> {
        let timing = self.timing;
        let boundary = timing.retrace_dot()?;
        let total = timing.frame_dots();
        let mut dots = (boundary + total - self.dot) % total;
        if dots == 0 {
            dots = total;
        }
        self.updated
            .checked_add(timing.remaining_for_dots(dots, self.dot_remainder))
    }

    pub fn input_status_1(&self) -> u8 {
        let timing = self.timing;
        let horizontal = self.dot % timing.horizontal_total;
        let vertical = self.dot / timing.horizontal_total / timing.scanlines_per_vertical_count;
        // Input Status 1 reports inverted display enable, rather than the
        // separate blanking signal or the narrower horizontal sync pulse.
        let mut status = 0;
        if timing.reset_active
            || horizontal < timing.horizontal_display_skew
            || horizontal >= timing.horizontal_display_skew + timing.horizontal_display
            || vertical >= timing.vertical_display
        {
            status |= DISPLAY_DISABLED;
        }
        if timing.vertical_retrace_active(vertical) {
            status |= VERTICAL_RETRACE;
        }
        status
    }
}
