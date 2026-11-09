//! Execution-local control of the next retirement checkpoint.

use super::{Cpu, RuntimeMemory};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use relocade_x86::ExecutionContext;
use std::{mem::offset_of, rc::Rc};

/// Mutable instruction stop target, independent of a CPU run future.
/// Capture a clone before running the CPU to update it from host services or
/// synchronous device callbacks. One execution owner must coordinate writes.
/// Dropping a handle does not clear the target; its controller owns that lifetime.
#[derive(Clone)]
pub struct InstructionTarget(Rc<State>);

struct State {
    storage: SharedMemory,
    enabled: bool,
}

impl InstructionTarget {
    pub(super) fn new(memory: &RuntimeMemory) -> Self {
        Self(Rc::new(State {
            storage: memory.context_backing.clone(),
            enabled: memory
                .configuration()
                .execution_target()
                .instruction_budgets,
        }))
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
        self.0.storage.view().store(
            offset_of!(ExecutionContext, instruction_target) as u32,
            target.map_or(0, |target| target.max(1)),
        );
    }
}

impl Cpu {
    /// Retains execution-local control of the instruction checkpoint target.
    pub fn instruction_target(&self) -> InstructionTarget {
        self.instruction_target.clone()
    }
}
