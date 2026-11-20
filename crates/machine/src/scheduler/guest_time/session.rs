//! Switches the live session's host timer between CPU calibration and idle device waits.

use super::{GuestTimeDriver, Realtime};
use crate::Timestamp;
use std::{future::Future, pin::Pin, task::Context};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Alarm {
    Sample,
    Device(Timestamp),
}

impl Realtime {
    fn set_running(&mut self, running: bool, retired: u64) -> bool {
        if self.running == running {
            return false;
        }
        self.alarm = None;
        self.sample(retired, running);
        true
    }

    /// Returns true if the new alarm already elapsed before its first poll.
    fn arm(&mut self, earliest: Option<Timestamp>, cx: &mut Context<'_>) -> bool {
        let next = if self.running {
            Some(Alarm::Sample)
        } else {
            earliest.map(Alarm::Device)
        };
        if next == self.alarm.as_ref().map(|(at, _)| *at) {
            return false;
        }
        self.alarm = None;
        let Some(at) = next else { return false };
        let mut timer = match at {
            Alarm::Sample => self.sample_timer(),
            Alarm::Device(deadline) => self.sleep_until(deadline),
        };
        let elapsed = Pin::new(&mut timer).poll(cx).is_ready();
        self.alarm = Some((at, timer));
        elapsed
    }
}

impl GuestTimeDriver {
    pub(in crate::scheduler) fn cpu_boundary(&self, continues: bool) -> bool {
        {
            let mut state = self.0.borrow_mut();
            if let Some((live, count)) = state.live_clock() {
                live.cpu_continues = continues && (live.running || live.estimate.retired != count);
                if !continues && live.set_running(false, count) {
                    state.refresh_target();
                }
            }
        }
        self.synchronize()
    }
}

pub(in crate::scheduler) struct BindingGuard {
    pub(super) driver: GuestTimeDriver,
}

impl BindingGuard {
    /// Sample an idle wake or a completed calibration before polling device tasks.
    pub(in crate::scheduler) fn observe(&mut self, cx: &mut Context<'_>) {
        let mut state = self.driver.0.borrow_mut();
        let Some((live, count)) = state.live_clock() else {
            return;
        };
        if live.running {
            let Some((Alarm::Sample, timer)) = &mut live.alarm else {
                return;
            };
            if Pin::new(timer).poll(cx).is_pending() {
                return;
            }
            live.alarm = None;
        }
        live.sample(count, live.running);
        state.refresh_target();
        drop(state);
        self.driver.synchronize();
    }

    /// A checkpoint schedules another CPU poll. A foreground future can also
    /// stop polling the CPU; its devices then need their earliest idle alarm.
    pub(in crate::scheduler) fn arm(&mut self, cx: &mut Context<'_>) {
        let mut state = self.driver.0.borrow_mut();
        let earliest = state.waits.earliest;
        let Some((live, count)) = state.live_clock() else {
            return;
        };
        let continues = std::mem::take(&mut live.cpu_continues);
        let changed = live.set_running(continues, count);
        let elapsed = live.arm(earliest, cx);
        if changed {
            state.refresh_target();
        }
        drop(state);
        if elapsed {
            cx.waker().wake_by_ref();
        }
    }
}

impl Drop for BindingGuard {
    fn drop(&mut self) {
        let mut state = self.driver.0.borrow_mut();
        let now = state.now();
        if let Some(live) = &mut state.realtime {
            live.alarm = None;
            live.cpu_continues = false;
        }
        state.cpu.take().unwrap().state.set_instruction_target(None);
        state.offset = now.ticks();
    }
}
