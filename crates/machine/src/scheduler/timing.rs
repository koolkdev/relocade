//! Device deadlines, idle time and the optional CPU throughput limit.

mod slice;
use slice::{CpuLimit, ExecutionSlice};

use super::GuestTimeConfig;
use crate::{Clock, Cpu, GuestTimeDriver, Sleep, Timestamp};
use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{ready, Context, Poll, Waker},
};

pub(crate) struct Timing {
    clock: Clock,
    guest: Option<GuestSchedule>,
}

struct GuestSchedule {
    driver: GuestTimeDriver,
    slice: Option<ExecutionSlice>,
    host: Option<Clock>,
    limit: Option<CpuLimit>,
    idle: Option<IdleWait>,
}

// Keep the same host origin when a device replaces its wait while idle.
struct IdleWait {
    guest_start: Timestamp,
    host_start: Timestamp,
    deadline: Timestamp,
    sleep: Sleep,
}

impl Timing {
    pub(crate) fn from_clock(clock: Clock) -> Self {
        Self { clock, guest: None }
    }

    pub(crate) fn with_guest_time(config: GuestTimeConfig) -> Self {
        let mut driver = GuestTimeDriver::new(config.instructions_per_second);
        if let Some(host) = &config.host_clock {
            driver = driver.with_host_clock(host.clone());
        }
        let clock = driver.clock();
        let slice = config
            .slice_duration
            .map(|duration| ExecutionSlice::new(&clock, duration));
        Self {
            clock,
            guest: Some(GuestSchedule {
                driver,
                slice,
                host: config.host_clock,
                limit: config.cpu_limit.map(CpuLimit::new),
                idle: None,
            }),
        }
    }

    pub(crate) fn clock(&self) -> &Clock {
        &self.clock
    }

    #[must_use = "retain the exit and deadline wake bindings for the session"]
    pub(super) fn bind(
        &self,
        cpu: &Cpu,
        changed: &Waker,
    ) -> (Rc<Waker>, Option<impl Drop + use<>>) {
        let deadlines = self
            .guest
            .as_ref()
            .map(|guest| guest.driver.bind(cpu, changed));
        (cpu.exit_handle().bind(changed), deadlines)
    }

    /// A CPU limit applies only to instructions that actually retired. An idle
    /// jump changes guest time without consuming any of the remaining burst.
    pub(super) fn poll_pacing(&mut self, cpu: &Cpu, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(guest) = &mut self.guest {
            let now = self.clock.now();
            let mut target = guest.slice.as_mut().map(|slice| slice.next(now));
            if let Some(limit) = &mut guest.limit {
                let remaining = ready!(limit.poll(
                    guest.host.as_ref().unwrap(),
                    cpu.cpu_state().instruction_count(),
                    cx
                ));
                let end = Timestamp::from_ticks(now.ticks().wrapping_add(remaining));
                target = Some(target.map_or(end, |slice| slice.min(end)));
            }
            guest.driver.set_yield_deadline(target);
        }
        Poll::Ready(())
    }

    pub(super) fn notify_due(&mut self) {
        if let Some(guest) = &mut self.guest {
            if guest
                .idle
                .as_ref()
                .is_some_and(|idle| idle.guest_start != self.clock.now())
            {
                guest.idle = None;
            }
            guest.driver.notify_due();
        }
    }

    /// Wait for the next device when attached to a live host. A deterministic
    /// embedding can advance directly. Both visit guest deadlines in order.
    pub(super) fn advance_idle(&mut self, cx: &mut Context<'_>) -> bool {
        let Some(guest) = &mut self.guest else {
            return false;
        };
        let Some(deadline) = guest.driver.next_deadline() else {
            guest.idle = None;
            return false;
        };
        if let Some(host) = &guest.host {
            let now = self.clock.now();
            if guest
                .idle
                .as_ref()
                .is_none_or(|idle| (idle.guest_start, idle.deadline) != (now, deadline))
            {
                let host_start = guest
                    .idle
                    .as_ref()
                    .filter(|idle| idle.guest_start == now)
                    .map_or_else(|| host.now(), |idle| idle.host_start);
                let delay = self.clock.tick_rate().duration_between_ceil(now, deadline);
                let end = host.deadline_after(host_start, delay);
                guest.idle = Some(IdleWait {
                    guest_start: now,
                    host_start,
                    deadline,
                    sleep: host.sleep_until_if_needed(end),
                });
            }
            if Pin::new(&mut guest.idle.as_mut().unwrap().sleep)
                .poll(cx)
                .is_pending()
            {
                return false;
            }
            guest.idle = None;
        }
        guest.driver.advance_to(deadline);
        true
    }
}
