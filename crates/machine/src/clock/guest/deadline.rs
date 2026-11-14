//! Instruction-count comparisons for synchronous guest-clock polling.

use super::State;
use crate::{ClockDeadline, Timestamp};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(super) struct Deadline {
    pub(super) state: Rc<RefCell<State>>,
    pub(super) deadline: Cell<Option<Timestamp>>,
    pub(super) target: Cell<Option<Target>>,
}

#[derive(Clone, Copy)]
pub(super) struct Target {
    revision: u64,
    count: u64,
}

impl Target {
    fn prepare(state: &State, deadline: Timestamp) -> Option<Self> {
        Some(Self {
            revision: state.timeline_revision,
            count: state.instruction_target(deadline)?,
        })
    }
}

impl ClockDeadline for Deadline {
    fn get(&self) -> Option<Timestamp> {
        self.deadline.get()
    }

    fn set(&self, deadline: Option<Timestamp>) {
        self.deadline.set(deadline);
        self.target
            .set(deadline.and_then(|at| Target::prepare(&self.state.borrow(), at)));
    }

    fn now_if_due(&self) -> Option<Timestamp> {
        {
            let state = self.state.borrow();
            if let Some(target) = self
                .target
                .get()
                .filter(|target| target.revision == state.timeline_revision)
            {
                let cpu = state.cpu.as_ref().expect("prepared instruction target");
                let count = cpu.state.instruction_count();
                if (count.wrapping_sub(target.count) as i64) < 0 {
                    return None;
                }
            }
        }
        let deadline = self.deadline.get()?;
        let mut state = self.state.borrow_mut();
        let now = state.observe();
        // Far deadlines use bounded instruction targets. Reaching an intermediate
        // target rearms the comparison without reporting the deadline early.
        self.target.set(Target::prepare(&state, deadline));
        (now >= deadline).then_some(now)
    }
}
