use relocade_machine::{Clock, Timestamp};
use std::time::Duration;

/// Reloading eight-bit timer. Masking clears its flag without stopping its count.
pub(super) struct Timer {
    pub reload: u8,
    pub overflow: bool,
    masked: bool,
    tick: Duration,
    deadline: Option<Timestamp>,
}

impl Timer {
    pub fn new(tick_micros: u64) -> Self {
        Self {
            reload: 0,
            overflow: false,
            masked: false,
            tick: Duration::from_micros(tick_micros),
            deadline: None,
        }
    }

    fn period(&self) -> Duration {
        self.tick * (256 - u32::from(self.reload))
    }

    pub fn control(&mut self, clock: &Clock, now: Timestamp, start: bool, masked: bool) {
        self.masked = masked;
        if masked {
            self.overflow = false;
        }
        match (start, self.deadline) {
            (true, None) => self.deadline = Some(clock.add(now, self.period())),
            (false, _) => self.deadline = None,
            _ => {}
        }
    }

    pub fn update(&mut self, clock: &Clock, now: Timestamp) {
        let Some(deadline) = self.deadline.filter(|&deadline| deadline <= now) else {
            return;
        };
        self.overflow |= !self.masked;
        // Reload writes affect the next overflow. Skip elapsed periods in one
        // step, retaining the original timer phase even after a long idle gap.
        let elapsed = clock.duration_since(now, deadline);
        let period = self.period();
        let remainder = Duration::from_nanos((elapsed.as_nanos() % period.as_nanos()) as u64);
        self.deadline = Some(clock.add(deadline, elapsed - remainder + period));
    }
}
