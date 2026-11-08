//! Execution-local time estimated from published instruction retirement.

use super::{Clock, Sleep, Source, Timestamp};
use crate::CpuStateAccess;
use futures_channel::oneshot;
use std::{cell::RefCell, num::NonZeroU64, rc::Rc, time::Duration};

/// A fixed-rate guest clock shared by devices on one execution owner.
/// Time starts at zero. Bind the session CPU after device setup and before running
/// guest code. Reads use its published instruction count, which may lag execution
/// during synchronous device callbacks. The count must not be reset while bound.
///
/// A scheduler calls [`Self::notify_due`] at execution boundaries and can use
/// [`Self::advance_to`] while the guest is idle. This clock starts no host timers
/// and does not choose CPU budgets or pace execution against real time.
#[derive(Clone)]
pub struct InstructionTimeline(Rc<RefCell<State>>);

struct State {
    rate: u64,
    cpu: Option<CpuStateAccess>,
    count: u64,
    now: Timestamp,
    remainder: u64,
    waits: Vec<Wait>,
}

struct Wait {
    deadline: Timestamp,
    complete: oneshot::Sender<()>,
}

impl InstructionTimeline {
    pub fn new(instructions_per_second: NonZeroU64) -> Self {
        Self(Rc::new(RefCell::new(State {
            rate: instructions_per_second.get(),
            cpu: None,
            count: 0,
            now: Timestamp::ZERO,
            remainder: 0,
            waits: Vec::new(),
        })))
    }

    pub fn clock(&self) -> Clock {
        Clock::new(self.clone())
    }

    /// Attaches the session CPU without counting its preexisting retirement.
    /// Pending device waits and elapsed idle time keep their original timebase.
    /// Call once, on the CPU's execution owner, before guest execution starts.
    pub fn bind_cpu(&self, cpu: CpuStateAccess) {
        let mut state = self.0.borrow_mut();
        assert!(state.cpu.is_none(), "guest clock already has a CPU");
        state.count = cpu.instruction_count();
        state.cpu = Some(cpu);
    }

    /// Earliest registered wait, including overdue waits until they are notified.
    /// Dropped wait futures no longer constrain the next deadline.
    pub fn next_deadline(&self) -> Option<Timestamp> {
        let mut state = self.0.borrow_mut();
        state.waits.retain(|wait| !wait.complete.is_canceled());
        state.waits.iter().map(|wait| wait.deadline).min()
    }

    /// Samples CPU progress and wakes due waits. Poll their activities before
    /// choosing the next CPU budget, so they can register subsequent deadlines.
    pub fn notify_due(&self) {
        let due = {
            let mut state = self.0.borrow_mut();
            let now = state.observe();
            state
                .waits
                .extract_if(.., |wait| {
                    wait.deadline <= now || wait.complete.is_canceled()
                })
                .collect::<Vec<_>>()
        };
        for wait in due {
            let _ = wait.complete.send(());
        }
    }

    /// Moves idle guest time forward and notifies due waits. A past deadline
    /// leaves time unchanged; fractional CPU progress survives the idle advance.
    pub fn advance_to(&self, deadline: Timestamp) {
        {
            let mut state = self.0.borrow_mut();
            state.now = state.observe().max(deadline);
        }
        self.notify_due();
    }
}

impl Source for InstructionTimeline {
    fn now(&self) -> Timestamp {
        self.0.borrow_mut().observe()
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        let timeline = self.clone();
        Sleep::new(async move {
            if deadline <= timeline.now() {
                return;
            }
            let (complete, completed) = oneshot::channel();
            timeline
                .0
                .borrow_mut()
                .waits
                .push(Wait { deadline, complete });
            completed
                .await
                .expect("instruction timeline owns pending sleeps");
        })
    }
}

impl State {
    fn observe(&mut self) -> Timestamp {
        if let Some(cpu) = &self.cpu {
            let count = cpu.instruction_count();
            let retired = count.wrapping_sub(self.count);
            let scaled = u128::from(retired) * 1_000_000_000 + u128::from(self.remainder);
            let nanos = scaled / u128::from(self.rate);
            self.remainder = (scaled % u128::from(self.rate)) as u64;
            self.now = self.now.saturating_add(Duration::new(
                (nanos / 1_000_000_000) as u64,
                (nanos % 1_000_000_000) as u32,
            ));
            self.count = count;
        }
        self.now
    }
}
