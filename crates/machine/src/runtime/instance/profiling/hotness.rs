//! Runtime admission from accumulated CPU-local call observations.

use relocade_x86::CallCount;
use std::collections::{HashMap, VecDeque};

const HOT_CALLS: u64 = 1024;

#[derive(Default)]
pub(super) struct Hotness {
    counts: HashMap<u32, u64>,
    candidates: VecDeque<u32>,
}

impl Hotness {
    pub(super) fn record(&mut self, entry: CallCount) {
        let count = self.counts.entry(entry.target).or_default();
        let previous = *count;
        *count = count.saturating_add(u64::from(entry.count));
        // Queue only the threshold crossing. Continued interpreted execution
        // while compilation is pending cannot create duplicate requests.
        if previous < HOT_CALLS && *count >= HOT_CALLS {
            self.candidates.push_back(entry.target);
        }
    }

    pub(super) fn count(&self, key: u32) -> u64 {
        self.counts.get(&key).copied().unwrap_or(0)
    }

    pub(super) fn next_candidate(&mut self) -> Option<u32> {
        self.candidates.pop_front()
    }
}

#[cfg(test)]
mod tests;
