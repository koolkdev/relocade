//! Device deadlines, idle time and the optional CPU throughput limit.

mod cpu_limit;
use cpu_limit::CpuLimit;

use super::{GuestTimeConfig, GuestTimeDriver};
use crate::{Clock, Cpu, Sleep, Timestamp};
use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll, Waker},
};

pub(crate) struct Timing {
    clock: Clock,
    guest: Option<GuestSchedule>,
}

struct GuestSchedule {
    driver: GuestTimeDriver,
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
        Self {
            clock,
            guest: Some(GuestSchedule {
                driver,
                host: config.host_clock,
                limit: config.cpu_limit.map(CpuLimit::new),
                idle: None,
            }),
        }
    }

    pub(crate) fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Keep the guest clock attached across CPU runs and idle foreground work.
    pub(crate) fn bind(&self, cpu: &Cpu, changed: &Waker) -> Option<impl Drop + use<>> {
        self.guest
            .as_ref()
            .map(|guest| guest.driver.bind(cpu, changed))
    }

    /// A CPU limit applies only to instructions that actually retired. An idle
    /// jump changes guest time without consuming any of the remaining burst.
    pub(super) fn poll_pacing(&mut self, cpu: &Cpu, cx: &mut Context<'_>) -> Poll<()> {
        let Some(guest) = &mut self.guest else {
            return Poll::Ready(());
        };
        let target = if let Some(limit) = &mut guest.limit {
            let remaining = ready!(limit.poll(
                guest.host.as_ref().unwrap(),
                cpu.cpu_state().instruction_count(),
                cx
            ));
            Some(Timestamp::from_ticks(
                self.clock.now().ticks().wrapping_add(remaining),
            ))
        } else {
            None
        };
        guest.driver.set_yield_deadline(target);
        Poll::Ready(())
    }

    pub(super) fn notify_due(&mut self) -> bool {
        if let Some(guest) = &mut self.guest {
            if guest
                .idle
                .as_ref()
                .is_some_and(|idle| idle.guest_start != self.clock.now())
            {
                guest.idle = None;
            }
            return guest.driver.notify_due();
        }
        false
    }

    /// Wait for the next device when attached to a live host. A deterministic
    /// embedding can advance directly. Both visit guest deadlines in order.
    pub(super) fn advance_idle(&mut self, cx: &mut Context<'_>) -> bool {
        let Some(guest) = &mut self.guest else {
            return false;
        };
        let Some(deadline) = guest.driver.idle_deadline(cx.waker()) else {
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
