//! Calibrates live device time against the host while the CPU is running.

use super::{
    interpolation::{Interpolation, ONE_TICK},
    session::Alarm,
};
use crate::{Clock, Sleep, TickRate, Timestamp};
use std::time::Duration;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(4);
// The first measurement replaces this startup estimate. It never limits the CPU.
const INITIAL_INSTRUCTIONS_PER_SECOND: u64 = 50_000_000;

pub(super) struct Realtime {
    host: Clock,
    rate: TickRate,
    host_start: Timestamp,
    guest_start: Timestamp,
    sample_host: Timestamp,
    host_window: u64,
    guest_window: u128,
    instructions_per_window: u64,
    pub(super) estimate: Interpolation,
    pub(super) running: bool,
    pub(super) cpu_continues: bool,
    pub(super) alarm: Option<(Alarm, Sleep)>,
}

impl Realtime {
    pub(super) fn new(host: Clock, rate: TickRate) -> Self {
        let zero = Timestamp::from_ticks(0);
        let host_window = host.deadline_after(zero, SAMPLE_INTERVAL).ticks();
        let host_rate = u128::from(host.tick_rate().ticks_per_second().get());
        let guest_rate = u128::from(rate.ticks_per_second().get());
        let scaled = u128::from(host_window) * guest_rate;
        let guest_window =
            (scaled / host_rate) * ONE_TICK + (scaled % host_rate) * ONE_TICK / host_rate;
        Self {
            host,
            rate,
            host_start: zero,
            guest_start: zero,
            sample_host: zero,
            host_window,
            guest_window,
            estimate: Interpolation::default(),
            running: false,
            cpu_continues: false,
            alarm: None,
            instructions_per_window: (u128::from(host_window)
                * u128::from(INITIAL_INSTRUCTIONS_PER_SECOND)
                / host_rate)
                .max(1) as u64,
        }
    }

    pub(super) fn start(&mut self, guest: Timestamp, retired: u64) {
        self.host_start = self.host.now();
        self.guest_start = guest;
        self.running = false;
        self.sample_host = self.host_start;
        let phase = u128::from(guest.ticks()) * ONE_TICK;
        self.estimate.anchor(
            retired,
            phase,
            phase,
            self.guest_window,
            self.instructions_per_window,
        );
    }

    /// Idle time follows the host. During execution, materialize the old slope
    /// first so that changing the rate affects only subsequent retirement.
    pub(super) fn sample(&mut self, retired: u64, running: bool) {
        let host = self.host.now();
        let (ticks, remainder) = self.position(host);
        let wall = ticks * ONE_TICK
            + remainder * ONE_TICK / u128::from(self.host.tick_rate().ticks_per_second().get());
        let elapsed = host.ticks().wrapping_sub(self.sample_host.ticks());
        let count = retired.wrapping_sub(self.estimate.retired);
        if self.running && elapsed != 0 && count != 0 {
            self.instructions_per_window = (u128::from(count) * u128::from(self.host_window)
                / u128::from(elapsed))
            .clamp(1, i64::MAX as u128) as u64;
        }
        let phase = self.estimate.phase(retired);
        let phase = if self.running && running && count != 0 {
            phase
        } else {
            phase.max(wall)
        };
        self.estimate.anchor(
            retired,
            phase,
            wall,
            self.guest_window,
            self.instructions_per_window,
        );
        self.sample_host = host;
        self.running = running;
    }

    pub(super) fn sample_timer(&self) -> Sleep {
        let at = Timestamp::from_ticks(self.sample_host.ticks().wrapping_add(self.host_window));
        self.host.sleep_until(at)
    }

    /// Whole guest ticks and the remainder in units of the host tick rate.
    fn position(&self, host: Timestamp) -> (u128, u128) {
        let elapsed = host.ticks().wrapping_sub(self.host_start.ticks());
        let scaled = u128::from(elapsed) * u128::from(self.rate.ticks_per_second().get());
        let host_rate = u128::from(self.host.tick_rate().ticks_per_second().get());
        (
            u128::from(self.guest_start.ticks()) + scaled / host_rate,
            scaled % host_rate,
        )
    }

    pub(super) fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        let host_now = self.host.now();
        let host_rate = u128::from(self.host.tick_rate().ticks_per_second().get());
        let guest_rate = u128::from(self.rate.ticks_per_second().get());
        let (now, fraction) = self.position(host_now);
        let now = Timestamp::from_ticks(now as u64);
        if deadline <= now {
            return Sleep::new(async {});
        }
        // Retain the fraction of the current guest tick when arming a host timer.
        let remaining = u128::from(deadline.ticks().wrapping_sub(now.ticks()));
        let ticks = (remaining * host_rate - fraction).div_ceil(guest_rate);
        let host = self.host.clone();
        Sleep::new(async move {
            let mut start = host_now;
            let mut left = ticks;
            // Two host timestamps can be compared within half a counter range.
            while left != 0 {
                let span = left.min(i64::MAX as u128);
                start = Timestamp::from_ticks(start.ticks().wrapping_add(span as u64));
                host.sleep_until_if_needed(start).await;
                left -= span;
            }
        })
    }
}
