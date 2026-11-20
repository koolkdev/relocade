//! Guest device deadlines and the optional CPU throughput limit.

mod cpu_limit;
use cpu_limit::CpuLimit;

use super::{guest_time::BindingGuard, GuestTimeConfig, GuestTimeDriver};
use crate::{Clock, Cpu};
use std::task::{Context, Poll, Waker};

pub(crate) struct Timing {
    clock: Clock,
    guest: Option<GuestTiming>,
}

struct GuestTiming {
    driver: GuestTimeDriver,
    limit: Option<CpuLimit>,
}

impl Timing {
    pub(crate) fn from_clock(clock: Clock) -> Self {
        Self { clock, guest: None }
    }

    pub(crate) fn with_guest_time(config: GuestTimeConfig, host: Clock) -> Self {
        let limit = config
            .cpu_limit
            .map(|rate| CpuLimit::new(rate, host.clone()));
        let driver = GuestTimeDriver::new(config.ticks_per_second).with_host_clock(host);
        Self {
            clock: driver.clock(),
            guest: Some(GuestTiming { driver, limit }),
        }
    }

    pub(crate) fn clock(&self) -> &Clock {
        &self.clock
    }

    pub(crate) fn bind(&self, cpu: &Cpu, changed: &Waker) -> Option<BindingGuard> {
        self.guest
            .as_ref()
            .map(|guest| guest.driver.start_session(cpu, changed))
    }

    pub(super) fn poll_pacing(&mut self, cpu: &Cpu, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(GuestTiming {
            driver,
            limit: Some(limit),
        }) = &mut self.guest
        {
            let retired = cpu.cpu_state().instruction_count();
            let remaining = std::task::ready!(limit.allowance(retired, cx));
            driver.set_pacing_target(retired.wrapping_add(remaining));
        }
        Poll::Ready(())
    }

    pub(super) fn cpu_boundary(&self, cpu: &Cpu, checkpoint: bool) -> bool {
        self.guest.as_ref().is_some_and(|guest| {
            let continues = checkpoint
                && !cpu.is_blocked()
                && guest
                    .limit
                    .as_ref()
                    .is_none_or(|limit| limit.remaining(cpu.cpu_state().instruction_count()) != 0);
            guest.driver.cpu_boundary(continues)
        })
    }
}
