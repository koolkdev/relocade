//! Instruction allowances live for one run waiter, including its pending polls.

use super::super::RuntimeMemory;
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use relocade_x86::ExecutionContext;
use std::mem::offset_of;

pub(super) struct RunBudget(Option<SharedMemory>);

impl RunBudget {
    pub(super) fn new(memory: &RuntimeMemory, allowance: u64) -> Self {
        assert!(
            allowance <= i64::MAX as u64,
            "instruction budget is too large"
        );
        if allowance == 0 {
            return Self(None);
        }
        assert!(
            memory
                .configuration()
                .execution_target()
                .instruction_budgets,
            "this execution target disables instruction budgets"
        );
        let storage = &memory.context_backing;
        let mut view = storage.view();
        let count = view.load::<u64>(offset_of!(ExecutionContext, cpu.instruction_count) as u32);
        // Zero is reserved for unlimited execution. Moving a wrapped zero to one
        // may postpone exhaustion to the next checkpoint, as soft budgets allow.
        let target = count.wrapping_add(allowance).max(1);
        view.store(
            offset_of!(ExecutionContext, instruction_target) as u32,
            target,
        );
        Self(Some(storage.clone()))
    }
}

impl Drop for RunBudget {
    fn drop(&mut self) {
        if let Some(storage) = &self.0 {
            storage.view().store(
                offset_of!(ExecutionContext, instruction_target) as u32,
                0u64,
            );
        }
    }
}
