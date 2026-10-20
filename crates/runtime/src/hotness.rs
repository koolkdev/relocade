//! Host control of the Wasm-owned interpreter-entry counters.

use crate::{HotEntry, Runtime, RuntimeBackend};
use relocade_x86::HotnessTable;
use std::num::NonZeroU16;

impl<B: RuntimeBackend> Runtime<B> {
    /// Enables or disables compilation requests, resetting previous heat and
    /// pending request. Disabled initially; `run_async` enables the configured
    /// threshold when a compiler and lifetime allowance are available. Already
    /// queued jobs are unaffected. Counters run wholly in Wasm and apply
    /// only to dispatch misses, never compiled links or interpreter guard fallback.
    pub fn set_hot_threshold(&mut self, threshold: Option<NonZeroU16>) {
        self.storage
            .access
            .write_memory(&self.hotness, 0, HotnessTable::new(threshold).as_bytes());
    }

    /// Current request threshold; None disables profiling.
    pub fn hot_threshold(&self) -> Option<NonZeroU16> {
        self.read_hotness().threshold()
    }

    /// Takes the most recent compilation request. Taking it does not retire an
    /// instruction. Declining it is safe: a fixed cooldown allows execution to
    /// continue before another request for the same identity is considered.
    pub fn take_hot_request(&mut self) -> Option<HotEntry> {
        let mut table = self.read_hotness();
        let request = table.take_request();
        self.storage
            .access
            .write_memory(&self.hotness, 0, table.as_bytes());
        request
    }

    fn read_hotness(&self) -> HotnessTable {
        let mut bytes = [0; HotnessTable::BYTE_LEN];
        self.storage
            .access
            .read_memory(&self.hotness, 0, &mut bytes);
        HotnessTable::from_bytes(bytes)
    }
}
