use crate::timing::Timing;
use relocade_machine::Timestamp;

const VERTICAL_RETRACE: u8 = 1 << 3;

/// Exact position at `updated`, with status valid until `next_update`.
/// Observations between events leave the position lazy. Timing changes materialize
/// it at `observed` under the old timing before remapping the counters.
pub(super) struct Raster {
    timing: Timing,
    updated: Timestamp,
    observed: Timestamp,
    dot: u64,
    /// Fractional dot position in the units maintained by `Timing`.
    dot_remainder: u64,
    pub retrace_count: u64,
    next_retrace: Option<Timestamp>,
    status: u8,
    next_update: Option<Timestamp>,
}

impl Raster {
    pub fn new(now: Timestamp, timing: Timing) -> Self {
        let mut raster = Self {
            timing,
            updated: now,
            observed: now,
            dot: 0,
            dot_remainder: 0,
            retrace_count: 0,
            next_retrace: None,
            status: 0,
            next_update: None,
        };
        raster.refresh_deadline();
        raster.refresh_status();
        raster
    }

    /// Update counters to `now`, returning the latest crossed retrace time.
    /// Multiple missed retraces contribute to the count without a per-frame loop.
    pub fn update_to(&mut self, now: Timestamp) -> Option<Timestamp> {
        self.observed = now;
        if self.next_update.is_none_or(|deadline| now < deadline) {
            return None;
        }
        let completed = self.advance_to(now);
        self.refresh_status();
        completed
    }

    /// Materialize the exact phase, including when a programming write occurs
    /// between observable edges. Stopped clocks still move their time anchor.
    fn advance_to(&mut self, now: Timestamp) -> Option<Timestamp> {
        let timing = self.timing;
        let elapsed = now.duration_since(self.updated);
        if elapsed.is_zero() {
            return None;
        }
        self.updated = now;
        if !timing.clock_running() {
            return None;
        }
        let dots = timing.elapsed_dots(elapsed, &mut self.dot_remainder);
        let frame_dots = timing.frame_dots();
        let completed = self
            .next_retrace
            .filter(|deadline| now >= *deadline)
            .map(|_| {
                let boundary = timing.retrace_dot().expect("Running capture boundary");
                let since_previous_boundary = (self.dot + frame_dots - boundary) % frame_dots;
                let advanced = u128::from(since_previous_boundary) + dots;
                let frames = advanced / u128::from(frame_dots);
                self.retrace_count = self
                    .retrace_count
                    .checked_add(u64::try_from(frames).expect("Display frame count fits in u64"))
                    .expect("Display frame sequence exhausted");
                let since_latest_boundary = (advanced % u128::from(frame_dots)) as u64;
                now - timing.elapsed_for_dots(since_latest_boundary, self.dot_remainder)
            });
        self.dot = ((u128::from(self.dot) + dots) % u128::from(frame_dots)) as u64;
        if completed.is_some() {
            self.refresh_deadline();
        }
        completed
    }

    /// Called after advancing elapsed time under the preceding configuration.
    /// Returns whether the activity must replace its current deadline.
    pub fn set_timing(&mut self, new: Timing) -> bool {
        let old = self.timing;
        if old == new {
            return false;
        }
        // update_to already handled every due capture; only the phase between
        // events can remain lazy here.
        self.advance_to(self.observed);
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
            let horizontal = horizontal / old.dots_per_horizontal_count
                * new.dots_per_horizontal_count
                + (horizontal % old.dots_per_horizontal_count)
                    .min(new.dots_per_horizontal_count - 1);
            self.dot = (line * new.horizontal_total + horizontal % new.horizontal_total)
                % new.frame_dots();
            self.dot_remainder = new.rescale_remainder(self.dot_remainder, old);
        }
        self.refresh_deadline();
        self.refresh_status();
        true
    }

    /// Begin a new display mode at the already serviced clock time. Preserve
    /// the lifetime retrace count so consumers can wait across mode switches.
    pub fn restart(&mut self, timing: Timing) {
        self.timing = timing;
        self.updated = self.observed;
        self.dot = 0;
        self.dot_remainder = 0;
        self.refresh_deadline();
        self.refresh_status();
    }

    pub fn next_retrace_at(&self) -> Option<Timestamp> {
        self.next_retrace
    }

    fn refresh_deadline(&mut self) {
        self.next_retrace = self
            .timing
            .retrace_dot()
            .and_then(|dot| self.next_dot_at(dot));
    }

    pub fn next_retrace_pulse_at(&self) -> Option<Timestamp> {
        self.timing.retrace_pulse_dot()?;
        self.next_retrace
    }

    fn next_dot_at(&self, boundary: u64) -> Option<Timestamp> {
        let total = self.timing.frame_dots();
        let mut dots = (boundary + total - self.dot) % total;
        if dots == 0 {
            dots = total;
        }
        self.updated
            .checked_add(self.timing.remaining_for_dots(dots, self.dot_remainder))
    }

    pub fn input_status_1(&self) -> u8 {
        self.status
    }

    pub fn in_retrace(&self) -> bool {
        self.status & VERTICAL_RETRACE != 0
    }

    fn refresh_status(&mut self) {
        self.status = self.timing.input_status_1(self.dot);
        let next_status = self.timing.next_status_in_dots(self.dot).and_then(|dots| {
            self.updated
                .checked_add(self.timing.remaining_for_dots(dots, self.dot_remainder))
        });
        // Internal captures still advance latches and blink when external sync
        // is suppressed, independently of the observable status bits.
        self.next_update = match (next_status, self.next_retrace) {
            (Some(status), Some(retrace)) => Some(status.min(retrace)),
            (status, retrace) => status.or(retrace),
        };
    }
}

#[cfg(test)]
mod tests;
