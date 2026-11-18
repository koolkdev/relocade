//! Execution-local guest time and instruction targets.

#[cfg(test)]
mod tests;

mod host;
use host::HostTime;

mod wait;
use wait::{Source, Wait};

use super::{Clock, GuestClock, GuestClockSource, TickRate, Timestamp};
use crate::{Cpu, CpuStateAccess, InstructionTarget};
use std::{cell::RefCell, num::NonZeroU64, rc::Rc, task::Waker};

/// Owns emulated guest time, pending sleeps and the CPU instruction target.
/// Published retirement advances time at the configured instruction rate. The
/// embedding owns CPU execution and calls [`Self::synchronize`] at stopped
/// boundaries. It also decides when idle time may advance to the next deadline.
/// Timed devices use the [`GuestClock`] returned by [`Self::guest_clock`].
///
/// Callback observations can lag unpublished retirement; instruction targets
/// remain soft checkpoint limits. Runner chooses pacing boundaries; the driver
/// retains the optional host/guest time correspondence.
pub struct GuestTimeDriver(Rc<RefCell<State>>);

struct State {
    tick_rate: TickRate,
    // While bound, guest ticks are the published instruction count plus this
    // offset. Between runs, the offset holds the current device time.
    offset: u64,
    cpu: Option<Binding>,
    // A live PC session can drive device deadlines while no CPU run owns them.
    parked: Option<Waker>,
    waits: Vec<Wait>,
    host: Option<HostTime>,
}

struct Binding {
    state: CpuStateAccess,
    target: InstructionTarget,
    yield_deadline: Option<Timestamp>,
    changed: Waker,
}

impl GuestTimeDriver {
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self(Rc::new(RefCell::new(State {
            tick_rate: TickRate::new(instructions_per_second),
            offset: 0,
            cpu: None,
            parked: None,
            waits: Vec::new(),
            host: None,
        })))
    }

    pub(crate) fn with_host_clock(self, clock: Clock) -> Self {
        let mut state = self.0.borrow_mut();
        state.host = Some(HostTime::new(clock, state.tick_rate));
        drop(state);
        self
    }

    pub fn clock(&self) -> Clock {
        self.guest_clock().into()
    }

    /// The device clock driven by this CPU's retired instructions and idle deadlines.
    pub fn guest_clock(&self) -> GuestClock {
        GuestClock::new(Source(self.0.clone()))
    }

    /// Observes CPU retirement and controls its target until the returned guard
    /// is dropped. Preexisting retirement is not charged to the clock. Dropping
    /// the guard records final progress, clears the target and releases the waker.
    /// Pending device waits stay registered. A session driving devices between
    /// bindings can advance to their deadlines without retiring instructions.
    ///
    /// `changed` wakes the embedding when waits change, including while a CPU
    /// service is pending. The embedding must be this CPU's only target writer.
    /// Panics if the driver already has a live binding.
    #[must_use = "retain the binding while driving this CPU"]
    pub fn bind<'a>(&'a self, cpu: &Cpu, changed: &Waker) -> impl Drop + use<'a> {
        let mut state = self.0.borrow_mut();
        assert!(state.cpu.is_none(), "guest time driver is already bound");
        let now = state.now();
        if state.parked.is_none() {
            state.resume_host(now);
        }
        let cpu_state = cpu.cpu_state();
        state.offset = now.ticks().wrapping_sub(cpu_state.instruction_count());
        state.cpu = Some(Binding {
            state: cpu_state,
            target: cpu.instruction_target(),
            yield_deadline: None,
            changed: changed.clone(),
        });
        state.refresh_target();
        BindingGuard(&self.0)
    }

    /// Keeps the host correspondence alive while the session can alternate
    /// between device deadlines and CPU execution.
    pub(crate) fn drive_while_parked(&self, changed: Waker) -> Option<impl Drop + '_> {
        let mut state = self.0.borrow_mut();
        if state.parked.is_some() {
            return None;
        }
        if state.cpu.is_none() {
            let now = state.now();
            state.resume_host(now);
        }
        state.parked = Some(changed);
        Some(ParkedGuard(&self.0))
    }

    pub(crate) fn cpu_is_bound(&self) -> bool {
        self.0.borrow().cpu.is_some()
    }

    /// Reads guest time, completes due sleeps and arms the CPU
    /// target. `yield_deadline` is an optional absolute guest time at which the
    /// embedding wants execution to yield. Wait changes preserve it until the next
    /// synchronization. With host pacing, lag over 20 ms resets the correspondence
    /// without moving guest time or device deadlines. Call after execution stops
    /// and after ready activities have had a turn.
    pub fn synchronize(&self, yield_deadline: Option<Timestamp>) {
        if let Some(cpu) = &mut self.0.borrow_mut().cpu {
            cpu.yield_deadline = yield_deadline;
        }
        self.notify_due();
    }

    /// Advances guest time to the earliest registered sleep and completes due sleeps.
    /// Returns false when no sleep exists. Call only after ready activities have
    /// been polled and execution is still idle. The yield deadline does not cause
    /// this operation to advance time.
    pub fn advance_to_next_deadline(&self) -> bool {
        let Some(deadline) = self.next_deadline() else {
            return false;
        };
        self.advance_to(deadline);
        true
    }

    /// Earliest registered sleep in guest time. Query at a stopped boundary
    /// after ready activities have had an opportunity to replace their waits.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        self.0.borrow().waits.iter().map(|wait| wait.deadline).min()
    }

    fn notify_due(&self) {
        let due = {
            let mut state = self.0.borrow_mut();
            let now = state.now();
            if let Some(host) = &mut state.host {
                host.discard_lag(now);
            }
            let due = state
                .waits
                .extract_if(.., |wait| wait.deadline <= now)
                .collect::<Vec<_>>();
            state.refresh_target();
            due
        };
        for wait in due {
            let _ = wait.complete.send(());
        }
    }

    /// Advance only after execution and ready activities have become idle.
    pub(crate) fn advance_to(&self, deadline: Timestamp) {
        {
            let mut state = self.0.borrow_mut();
            let now = state.now();
            if deadline > now {
                state.offset = state
                    .offset
                    .wrapping_add(deadline.ticks().wrapping_sub(now.ticks()));
            }
        }
        self.notify_due();
    }
}

impl GuestClockSource for Source {}

struct BindingGuard<'a>(&'a RefCell<State>);

impl Drop for BindingGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        let now = state.now();
        state.cpu.take().unwrap().target.set(None);
        state.offset = now.ticks();
        let parked = state.parked.clone();
        if parked.is_none() {
            state.pause_host();
        }
        drop(state);
        if let Some(parked) = parked {
            parked.wake();
        }
    }
}

struct ParkedGuard<'a>(&'a RefCell<State>);

impl Drop for ParkedGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        state.parked.take();
        if state.cpu.is_none() {
            state.pause_host();
        }
    }
}

impl State {
    fn driver_waker(&self) -> Option<Waker> {
        self.cpu
            .as_ref()
            .map(|cpu| &cpu.changed)
            .or(self.parked.as_ref())
            .cloned()
    }

    fn resume_host(&mut self, now: Timestamp) {
        if let Some(host) = &mut self.host {
            host.resume(now);
        }
    }

    fn pause_host(&mut self) {
        if let Some(host) = &mut self.host {
            host.pause();
        }
    }

    fn now(&self) -> Timestamp {
        let count = self
            .cpu
            .as_ref()
            .map_or(0, |cpu| cpu.state.instruction_count());
        Timestamp::from_ticks(count.wrapping_add(self.offset))
    }

    fn refresh_target(&mut self) {
        let now = self.now();
        let Some(cpu) = &self.cpu else { return };
        let deadline = self
            .waits
            .iter()
            .map(|wait| wait.deadline)
            .chain(cpu.yield_deadline)
            .min();
        let target = deadline.map(|deadline| deadline.max(now).ticks().wrapping_sub(self.offset));
        cpu.target.set(target);
    }
}
