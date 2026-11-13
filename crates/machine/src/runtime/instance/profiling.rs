//! Owns CPU-local profiling storage and consumes generated profiling events.

use super::{CpuInstance, ExecutionTurn, WasmRuntime};
use crate::{Clock, FarPointer, Result, Sleep};
use relocade_wasm_runtime::{Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, CallProfile, ExecutionExit};
use std::{future::Future, pin::Pin, task::Context, time::Duration};
mod hotness;
use hotness::Hotness;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(10);

pub(in crate::runtime) struct Profiling {
    pub(super) memory: Memory,
    hotness: Hotness,
    sample: Option<Sleep>,
}

impl Profiling {
    pub(super) fn new(wasm: &mut WasmRuntime) -> Result<Self> {
        Ok(Self {
            memory: wasm.create_memory(abi::CALL_PROFILE)?,
            hotness: Hotness::default(),
            sample: None,
        })
    }

    fn poll(&mut self, wasm: &mut WasmRuntime, clock: &Clock, cx: &mut Context<'_>) {
        let sample = self
            .sample
            .get_or_insert_with(|| clock.sleep(SAMPLE_INTERVAL));
        if Pin::new(sample).poll(cx).is_ready() {
            self.collect(wasm);
            let mut sample = clock.sleep(SAMPLE_INTERVAL);
            // Register the next deadline before guest execution resumes.
            let _ = Pin::new(&mut sample).poll(cx);
            self.sample = Some(sample);
        }
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
            self.hotness.record(entry);
        }
        batch.clear(|offset, value| memory.store(offset, value));
    }

    pub(in crate::runtime) fn next_candidate(&mut self) -> Option<FarPointer> {
        self.hotness.next_candidate().map(FarPointer::from_packed)
    }

    pub(in crate::runtime) fn count(&self, target: FarPointer) -> u64 {
        self.hotness.count(target.packed())
    }
}

impl CpuInstance {
    pub(in crate::runtime) fn poll_profiling(&mut self, clock: &Clock, cx: &mut Context<'_>) {
        self.runtime
            .with_runtime(|wasm| self.profiling.poll(wasm, clock, cx));
    }

    pub(in crate::runtime) fn call_count(&mut self, target: FarPointer) -> u64 {
        self.runtime.with_runtime(|wasm| {
            self.profiling.collect(wasm);
            self.profiling.count(target)
        })
    }
}

#[cfg(test)]
mod tests;
