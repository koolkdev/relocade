//! Execution-local control of the next retirement checkpoint.

use super::{Cpu, RuntimeMemory};
use relocade_wasm_runtime::SharedMemory;
use relocade_x86::ExecutionContext;
use std::{cell::Cell, mem::offset_of, rc::Rc};

/// Mutable instruction stop target, independent of a CPU run future.
/// Capture a clone before running the CPU to update it from host services or
/// synchronous device callbacks. A session can arm an earlier clock deadline.
/// Dropping a handle does not clear the target; its controller owns that lifetime.
#[derive(Clone)]
pub struct InstructionTarget(Rc<State>);

struct State {
    storage: SharedMemory,
    enabled: bool,
    caller: Cell<Option<u64>>,
    deadline: Cell<Option<u64>>,
}

const OFFSET: u32 = offset_of!(ExecutionContext, instruction_target) as u32;

impl InstructionTarget {
    pub(super) fn new(memory: &RuntimeMemory) -> Self {
        Self(Rc::new(State {
            storage: memory.context_backing.clone(),
            caller: Cell::new(None),
            deadline: Cell::new(None),
            enabled: memory
                .configuration()
                .execution_target()
                .instruction_budgets,
        }))
    }

    pub(super) fn caller_reached(&self, count: u64) -> bool {
        self.0
            .caller
            .get()
            .is_some_and(|target| count.wrapping_sub(target) as i64 >= 0)
    }

    pub(crate) fn set_deadline(&self, target: Option<u64>) {
        self.0.deadline.set(target.map(|target| target.max(1)));
        self.publish();
    }

    /// Sets an absolute wrapping retirement target, or clears it with `None`.
    /// Keep targets within `i64::MAX` instructions of the current count. Guest
    /// checkpoints can overshoot; callback reads can lag unpublished retirement.
    /// A target of zero is represented by one because zero disables checkpoints.
    /// Setting a target does not wake a suspended host service.
    ///
    /// # Panics
    /// Panics for `Some` when the execution profile disables instruction budgets.
    pub fn set(&self, target: Option<u64>) {
        assert!(
            target.is_none() || self.0.enabled,
            "this execution target disables instruction budgets"
        );
        self.0.caller.set(target.map(|target| target.max(1)));
        self.publish();
    }

    fn publish(&self) {
        let memory = &self.0.storage;
        let target = match (self.0.caller.get(), self.0.deadline.get()) {
            (Some(caller), Some(deadline)) => {
                let now: u64 =
                    memory.load(offset_of!(ExecutionContext, cpu.instruction_count) as u32);
                [caller, deadline]
                    .into_iter()
                    .min_by_key(|target| target.wrapping_sub(now) as i64)
            }
            (caller, deadline) => caller.or(deadline),
        };
        memory.store(OFFSET, target.unwrap_or(0));
    }
}

impl Cpu {
    /// Retains execution-local control of the instruction checkpoint target.
    pub fn instruction_target(&self) -> InstructionTarget {
        self.instruction_target.clone()
    }
}
