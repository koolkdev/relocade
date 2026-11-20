//! Samples the Real16 call profile and accumulates counts for hot-function discovery.

mod hotness;

use self::hotness::Hotness;
use crate::{runtime::WasmRuntime, Instant};
use relocade_wasm_runtime::{Memory, WasmMemoryAccess};
use relocade_x86::CallProfile;
use std::time::Duration;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(50);

pub(super) struct Profiling {
    pub(super) memory: Memory,
    pub(super) hotness: Hotness,
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
    pub(super) fn collection_due(&mut self, now: Instant) -> bool {
        let deadline = *self.next_collection.get_or_insert(now + SAMPLE_INTERVAL);
        if now < deadline {
            return false;
        }
        self.next_collection = Some(now + SAMPLE_INTERVAL);
        true
    }

    pub(super) fn collect(&mut self, wasm: &mut WasmRuntime) {
        let mut memory = wasm.memory(&self.memory);
        let batch = CallProfile::read(|offset| memory.load(offset));
        for entry in batch.iter() {
            self.hotness.record(entry);
        }
        batch.clear(|offset, value| memory.store(offset, value));
    }
}

#[cfg(test)]
mod tests;
