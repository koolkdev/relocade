//! Keep only knowledge that holds on every incoming path.

use super::{Bits, Facts, Range};
use crate::body::ValueTable;

#[cfg(test)]
mod tests;

impl Bits {
    fn common(self, other: Self) -> Self {
        let mask = self.mask & other.mask & !(self.value ^ other.value);
        Self {
            mask,
            value: self.value & mask,
        }
    }
}

impl Facts {
    /// A block parameter keeps only logical bits proved by every incoming argument.
    pub(in crate::place) fn merge_parameter<'a>(
        &mut self,
        table: &ValueTable,
        parameter: usize,
        incoming: impl Iterator<Item = (&'a Self, usize)>,
    ) {
        let bits = incoming
            .map(|(facts, argument)| facts.bits(table, argument))
            .reduce(Bits::common)
            .unwrap_or_default()
            .restrict(table[parameter].ty.mask());
        if bits.mask != 0 {
            self.assume_bits(parameter, bits.mask, bits.value);
        }
    }

    pub(in crate::place) fn retain_common(&mut self, other: &Self) {
        self.known.retain(|id, bits| {
            let incoming = other.known.get(id).copied().unwrap_or_default();
            *bits = bits.common(incoming);
            bits.mask != 0
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
