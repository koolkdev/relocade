//! Recompilation eligibility for installed instructions in shared backing storage.

use std::{
    collections::{btree_map::Entry, BTreeMap},
    ops::Range,
    sync::Arc,
};

#[derive(Default)]
pub(super) struct CodeHistory {
    latest_installation: u64,
    /// Installations present when each byte became eligible for its one retry.
    first_invalidation: BTreeMap<u32, u64>,
    pub(super) excluded: ExcludedCode,
}

impl CodeHistory {
    pub(super) fn next_installation(&mut self) -> u64 {
        self.latest_installation += 1;
        self.latest_installation
    }

    pub(super) fn invalidate(&mut self, instruction: Range<u64>, installation: u64) {
        for byte in instruction {
            if self.excluded.intersects(byte as u32, 1) {
                continue;
            }
            match self.first_invalidation.entry(byte as u32) {
                Entry::Vacant(entry) => {
                    // All current installations share the retry, including aliases.
                    entry.insert(self.latest_installation);
                }
                Entry::Occupied(entry) if installation > *entry.get() => {
                    entry.remove();
                    self.excluded.insert(byte);
                }
                Entry::Occupied(_) => {}
            }
        }
    }
}

/// A compiler reader can hold a snapshot while writes extend the live ranges.
#[derive(Clone, Default)]
pub(super) struct ExcludedCode(Arc<Vec<Range<u64>>>);

impl ExcludedCode {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn intersects(&self, backing: u32, len: usize) -> bool {
        if len == 0 || self.is_empty() {
            return false;
        }
        let start = u64::from(backing);
        let next = self.0.partition_point(|range| range.end <= start);
        self.0
            .get(next)
            .is_some_and(|range| range.start < start + len as u64)
    }

    fn insert(&mut self, byte: u64) {
        // Include adjacent ranges so snapshots stay compact for long patched sequences.
        let first = self.0.partition_point(|range| range.end < byte);
        if self.0.get(first).is_some_and(|range| range.contains(&byte)) {
            return;
        }
        let mut merged = byte..byte + 1;
        let ranges = Arc::make_mut(&mut self.0);
        let mut end = first;
        while end < ranges.len() && ranges[end].start <= merged.end {
            merged.start = merged.start.min(ranges[end].start);
            merged.end = merged.end.max(ranges[end].end);
            end += 1;
        }
        ranges.splice(first..end, [merged]);
    }
}
