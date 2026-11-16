use relocade_machine::{Clock, Schedule, Timestamp};
use std::time::Duration;

/// Reloading eight-bit timer. Masking clears its flag without stopping its count.
pub(super) struct Timer {
    pub reload: u8,
    pub overflow: bool,
    masked: bool,
    count_period: Duration,
    schedule: Option<Schedule>,
}

impl Timer {
    pub fn new(count_period_micros: u64) -> Self {
        Self {
            reload: 0,
            overflow: false,
            masked: false,
            count_period: Duration::from_micros(count_period_micros),
            schedule: None,
        }
    }

    fn period(&self) -> Duration {
        self.count_period * (256 - u32::from(self.reload))
    }

    pub fn control(&mut self, clock: &Clock, now: Timestamp, start: bool, masked: bool) {
        self.masked = masked;
        if masked {
            self.overflow = false;
        }
        match (start, self.schedule) {
            (true, None) => {
                let mut schedule = clock.schedule(now);
                schedule.advance(self.period());
                self.schedule = Some(schedule);
            }
            (false, _) => self.schedule = None,
            _ => {}
        }
    }

    pub fn update(&mut self, now: Timestamp) {
        let Some(mut schedule) = self.schedule.filter(|schedule| schedule.deadline() <= now) else {
            return;
        };
        self.overflow |= !self.masked;
        // Reload writes affect the next overflow. Skip elapsed periods in one
        // step, retaining the original timer phase even after a long idle gap.
        let elapsed = schedule.elapsed_until(now);
        let period = self.period();
        let remainder = Duration::from_nanos((elapsed.as_nanos() % period.as_nanos()) as u64);
        schedule.advance(elapsed - remainder + period);
        self.schedule = Some(schedule);
    }
}
