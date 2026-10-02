//! Keep only knowledge that holds on every incoming path.

use super::{Bits, Facts, Range};

#[cfg(test)]
mod tests;

impl Facts {
    pub(in crate::place) fn retain_common(&mut self, other: &Self) {
        self.known.retain(|id, bits| {
            let incoming = other.known.get(id).copied().unwrap_or_default();
            let mask = bits.mask & incoming.mask & !(bits.value ^ incoming.value);
            *bits = Bits {
                mask,
                value: bits.value & mask,
            };
            mask != 0
        });
        self.ranges.retain(|id, range| {
            let Some(incoming) = other.ranges.get(id) else {
                return false;
            };
            // Either path can reach the join, so the interval must cover both.
            *range = Range {
                minimum: range.minimum.min(incoming.minimum),
                maximum: range.maximum.max(incoming.maximum),
            };
            true
        });
        self.comparisons.retain_common(&other.comparisons);
        // Earlier inferences may depend on knowledge just discarded.
        self.computed.get_mut().clear();
    }
}
