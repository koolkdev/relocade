use crate::timing::Timing;
use relocade_machine::{Clock, Timestamp};
use std::{cell::Cell, rc::Rc};

const VERTICAL_RETRACE: u8 = 1 << 3;

/// Exact position at `updated`, with shared status valid until its next update.
/// Observations between events leave the position lazy. Timing changes materialize
/// it through whole nanoseconds at `observed` before remapping the counters.
pub(super) struct Raster {
    clock: Clock,
    timing: Timing,
    updated: Timestamp,
    dot: u64,
    /// Fractional dot position in the units maintained by `Timing`.
    dot_remainder: u64,
    pub retrace_count: u64,
    next_retrace: Option<Timestamp>,
    pub status: Rc<RasterStatus>,
}

/// Authoritative polling state shared with status-port readers. Only the raster
/// publishes status and deadlines; readers record time and service due edges.
pub(super) struct RasterStatus {
    clock: Clock,
    observed: Cell<Timestamp>,
    value: Cell<u8>,
    next_update: Cell<Option<Timestamp>>,
}

impl RasterStatus {
    pub fn observe_now(&self) -> bool {
        self.observe(self.clock.now())
    }

    /// Record observed time even between events, so a programming change can
    /// materialize the remaining phase under the old timing. Return whether
    /// the caller must service a status or capture edge before continuing.
    #[inline]
    pub fn observe(&self, now: Timestamp) -> bool {
        self.observed.set(now);
        self.next_update
            .get()
            .is_some_and(|deadline| now >= deadline)
    }

    pub fn input_status_1(&self) -> u8 {
        self.value.get()
    }
}

impl Raster {
    pub fn new(clock: Clock, timing: Timing) -> Self {
        let now = clock.now();
        let mut raster = Self {
            clock: clock.clone(),
            timing,
            updated: now,
            dot: 0,
            dot_remainder: 0,
            retrace_count: 0,
            next_retrace: None,
            status: Rc::new(RasterStatus {
                clock,
                observed: Cell::new(now),
                value: Cell::new(0),
                next_update: Cell::new(None),
            }),
        };
        raster.refresh_deadline();
        raster.refresh_status();
        raster
    }

    /// Service the observed edge and return the latest crossed retrace time.
    /// Multiple missed retraces contribute to the count without a per-frame loop.
    pub fn update_observed(&mut self) -> Option<Timestamp> {
        let completed = self.advance_to(self.status.observed.get());
        self.refresh_status();
        completed
    }

    /// Integrate through whole elapsed nanoseconds, retaining the native
    /// fraction for the next update. Programming takes effect at this consumed
    /// position. Stopped clocks still consume time without advancing the beam.
    fn advance_to(&mut self, now: Timestamp) -> Option<Timestamp> {
        let timing = self.timing;
        let elapsed = self.clock.consume_elapsed(&mut self.updated, now);
        if elapsed.is_zero() {
            return None;
        }
        if !timing.clock_running() {
            return None;
        }
        let dots = timing.elapsed_dots(elapsed, &mut self.dot_remainder);
        let frame_dots = timing.frame_dots();
        let completed = self
            .next_retrace
            .filter(|deadline| self.updated >= *deadline)
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
                self.clock.sub(
                    self.updated,
                    timing.elapsed_for_dots(since_latest_boundary, self.dot_remainder),
                )
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
        // The caller already serviced every due capture; only the phase between
        // events can remain lazy here.
        self.advance_to(self.status.observed.get());
        self.timing = new;
        if old.reset_active || new.reset_active {
            self.updated = self.status.observed.get();
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
        self.updated = self.status.observed.get();
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
        self.clock.checked_add(
            self.updated,
            self.timing.remaining_for_dots(dots, self.dot_remainder),
        )
    }

    pub fn in_retrace(&self) -> bool {
        self.status.input_status_1() & VERTICAL_RETRACE != 0
    }

    fn refresh_status(&mut self) {
        self.status.value.set(self.timing.input_status_1(self.dot));
        let next_status = self.timing.next_status_in_dots(self.dot).and_then(|dots| {
            self.clock.checked_add(
                self.updated,
                self.timing.remaining_for_dots(dots, self.dot_remainder),
            )
        });
        // Internal captures still advance latches and blink when external sync
        // is suppressed, independently of the observable status bits.
        self.status
            .next_update
            .set(match (next_status, self.next_retrace) {
                (Some(status), Some(retrace)) => Some(status.min(retrace)),
                (status, retrace) => status.or(retrace),
            });
    }
}

#[cfg(test)]
mod tests;
