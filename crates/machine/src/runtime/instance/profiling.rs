//! Owns CPU-local profiling storage and accumulates Real16 call observations.
//! Keys are packed 16:16 CS:IP values. Protected-mode dispatch does not record.

use super::WasmRuntime;
use crate::Result;
use relocade_wasm_runtime::{Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, CallProfile};
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

    pub(super) fn collect(&mut self, wasm: &mut WasmRuntime) {
        let mut memory = wasm.memory(&self.memory);
        let batch = CallProfile::read(|offset| memory.load(offset));
        for entry in batch.iter() {
            let count = self.counts.entry(entry.target).or_default();
            *count = count.saturating_add(u64::from(entry.count));
        }
        batch.clear(|offset, value| memory.store(offset, value));
    }
}

#[cfg(test)]
mod tests;
