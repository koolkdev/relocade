//! Owns CPU-local profiling storage and consumes generated profiling events.

use super::{CpuInstance, ExecutionTurn, WasmRuntime};
use crate::{FarPointer, Result};
use relocade_wasm_runtime::{Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, CallProfile, ExecutionExit};
use std::collections::HashMap;

pub(in crate::runtime) struct Profiling {
    pub(super) memory: Memory,
    counts: HashMap<u32, u64>,
}

impl Profiling {
    pub(super) fn new(wasm: &mut WasmRuntime) -> Result<Self> {
        Ok(Self {
            memory: wasm.create_memory(abi::CALL_PROFILE)?,
            counts: HashMap::new(),
        })
    }

    pub(super) fn finish(&mut self, wasm: &mut WasmRuntime, exit: ExecutionExit) -> ExecutionTurn {
        self.collect_if_required(wasm);
        match exit {
            ExecutionExit::CallProfileFull => ExecutionTurn::Yield,
            exit => ExecutionTurn::Exit(exit),
        }
    }

    /// Check pressure at every return: checkpoint and single-step exits can
    /// take precedence over CallProfileFull while leaving an overflow entry.
    pub(super) fn collect_if_required(&mut self, wasm: &mut WasmRuntime) {
        let required = {
            let memory = wasm.memory(&self.memory);
            CallProfile::requires_drain(|offset| memory.load(offset))
        };
        if required {
            self.collect(wasm);
        }
    }

    fn collect(&mut self, wasm: &mut WasmRuntime) {
        let mut memory = wasm.memory(&self.memory);
        let batch = CallProfile::read(|offset| memory.load(offset));
        for entry in batch.iter() {
            let count = self.counts.entry(entry.target).or_default();
            *count = count.saturating_add(u64::from(entry.count));
        }
        batch.clear(|offset, value| memory.store(offset, value));
    }

    pub(in crate::runtime) fn count(&self, target: FarPointer) -> u64 {
        self.counts.get(&target.packed()).copied().unwrap_or(0)
    }
}

impl CpuInstance {
    pub(in crate::runtime) fn call_count(&mut self, target: FarPointer) -> u64 {
        self.runtime.with_runtime(|wasm| {
            self.profiling.collect(wasm);
            self.profiling.count(target)
        })
    }
}

#[cfg(test)]
mod tests;
