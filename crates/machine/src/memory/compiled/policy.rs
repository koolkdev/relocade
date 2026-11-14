//! A new compiled version gets one retry before unstable backing stays interpreted.

use std::ops::Range;

const MAX_INVALIDATIONS: u8 = 2;

#[derive(Default)]
pub(super) struct Policy {
    regions: Vec<Region>,
}

struct Region {
    range: Range<u64>,
    invalidations: u8,
}

impl Policy {
    /// Each write may affect several versions/aliases of the same block. Merge
    /// those spans first, so one mutation consumes at most one retry per region.
    pub(super) fn invalidate(&mut self, mut ranges: Vec<Range<u64>>) {
        ranges.sort_unstable_by_key(|range| range.start);
        let mut changed: Vec<Range<u64>> = Vec::new();
        for range in ranges {
            if let Some(last) = changed.last_mut().filter(|last| range.start < last.end) {
                last.end = last.end.max(range.end);
            } else {
                changed.push(range);
            }
        }
        for mut range in changed {
            let mut previous = 0;
            self.regions.retain(|region| {
                if region.range.start >= range.end || range.start >= region.range.end {
                    return true;
                }
                previous = previous.max(region.invalidations);
                range.start = range.start.min(region.range.start);
                range.end = range.end.max(region.range.end);
                false
            });
            self.regions.push(Region {
                range,
                invalidations: previous.saturating_add(1).min(MAX_INVALIDATIONS),
            });
        }
        self.regions
            .sort_unstable_by_key(|region| region.range.start);
    }

    pub(super) fn excluded(&self) -> Vec<Range<u64>> {
        self.regions
            .iter()
            .filter(|region| region.invalidations >= MAX_INVALIDATIONS)
            .map(|region| region.range.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_count_once_and_changed_boundaries_keep_the_retry_budget() {
        let mut policy = Policy::default();
        policy.invalidate(vec![0x100..0x110, 0x100..0x110, 0x108..0x118]);
        assert!(policy.excluded().is_empty());
        policy.invalidate(vec![0x10c..0x120, 0x200..0x204]);
        assert_eq!(policy.excluded(), [0x100..0x120]);
    }
}
