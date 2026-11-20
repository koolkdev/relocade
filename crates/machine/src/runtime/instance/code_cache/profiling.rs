//! Collects real-mode call counts and selects unbound hot functions for compilation.

mod hotness;

use self::hotness::Hotness;
use super::{CodeCache, DispatchTable};
use crate::{runtime::WasmRuntime, FarPointer, Instant};
use relocade_wasm_runtime::{Memory, WasmMemoryAccess};
use relocade_x86::{CallProfile, ExecutionMode};
use std::time::Duration;

const COLLECTION_INTERVAL: Duration = Duration::from_millis(50);

pub(super) struct Profiling {
    pub(super) memory: Memory,
    hotness: Hotness,
    next_collection: Option<Instant>,
}

impl Profiling {
    pub(super) fn new(memory: Memory) -> Self {
        Self {
            memory,
            hotness: Hotness::default(),
            next_collection: None,
        }
    }

    /// Collect opportunistically after the interval, without timers or yields.
    fn collection_due(&mut self, now: Instant) -> bool {
        let deadline = *self
            .next_collection
            .get_or_insert(now + COLLECTION_INTERVAL);
        if now < deadline {
            return false;
        }
        self.next_collection = Some(now + COLLECTION_INTERVAL);
        true
    }

    fn collect(&mut self, wasm: &mut WasmRuntime) {
        let mut memory = wasm.memory(&self.memory);
        let batch = CallProfile::read(|offset| memory.load(offset));
        for entry in batch.iter() {
            self.hotness.record(entry);
        }
        batch.clear(|offset, value| memory.store(offset, value));
    }

    pub(super) fn take_hot_functions(&mut self, dispatch: &DispatchTable) -> Vec<FarPointer> {
        self.hotness
            .take_batch(|key| dispatch.contains(key))
            .into_iter()
            .map(FarPointer::from_packed)
            .collect()
    }

    // Bound calls bypass the recorder. Retirement makes fresh calls eligible again.
    pub(super) fn reset_counts(&mut self, submitted: &[FarPointer]) {
        self.hotness.reset_counts(submitted);
    }
}

impl CodeCache {
    pub(in crate::runtime) fn collect_call_profile_if_due(&mut self) {
        if self.compiler.is_some()
            && self.guest.bindings.cpu.compiler_config().entry.mode == ExecutionMode::Real
            && self.profiling.collection_due(Instant::now())
        {
            self.collect_call_profile();
        }
    }

    pub(in crate::runtime::instance) fn collect_call_profile(&mut self) {
        self.guest
            .runtime
            .with_runtime(|wasm| self.profiling.collect(wasm));
        self.submit_hot_functions();
    }
}

#[cfg(test)]
mod tests;
