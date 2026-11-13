//! Runtime admission from accumulated CPU-local call observations.

use relocade_x86::CallCount;
use std::{cmp::Reverse, collections::HashMap};

const HOT_CALLS: u64 = 1024;
const FORCE_CALLS: u64 = 4096;
const MIN_FUNCTIONS: usize = 64;
const MAX_FUNCTIONS: usize = 128;

#[derive(Default)]
pub(super) struct Hotness {
    counts: HashMap<u32, u64>,
    candidates: Vec<u32>,
}

impl Hotness {
    pub(super) fn record(&mut self, entry: CallCount) {
        let count = self.counts.entry(entry.target).or_default();
        let previous = *count;
        *count = count.saturating_add(u64::from(entry.count));
        // Queue only the threshold crossing. Continued interpreted execution
        // while compilation is pending cannot create duplicate requests.
        if previous < HOT_CALLS && *count >= HOT_CALLS {
            self.candidates.push(entry.target);
        }
    }

    #[cfg(test)]
    pub(super) fn count(&self, key: u32) -> u64 {
        self.counts.get(&key).copied().unwrap_or(0)
    }

    pub(super) fn take_batch(&mut self, mut is_resolved: impl FnMut(u32) -> bool) -> Vec<u32> {
        self.candidates.retain(|&key| !is_resolved(key));
        if self.candidates.len() < MIN_FUNCTIONS
            && !self
                .candidates
                .iter()
                .any(|key| self.counts[key] >= FORCE_CALLS)
        {
            return Vec::new();
        }
        // Only eligible candidates are ordered; cold entries are never scanned.
        // A forced batch includes its hottest function even after a large backlog.
        let counts = &self.counts;
        self.candidates
            .sort_unstable_by_key(|key| (Reverse(counts[key]), *key));
        let count = self.candidates.len().min(MAX_FUNCTIONS);
        self.candidates.drain(..count).collect()
    }
}

#[cfg(test)]
mod tests;
