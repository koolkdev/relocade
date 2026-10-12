//! Keep only knowledge that holds on every incoming path.

use super::{Bits, PathContext, Range, ValueAnalysis};
use crate::body::ValueTable;

#[cfg(test)]
mod tests;

impl Bits {
    pub(super) fn common(self, other: Self) -> Self {
        let mask = self.mask & other.mask & !(self.value ^ other.value);
        Self {
            mask,
            value: self.value & mask,
        }
    }
}

impl ValueAnalysis {
    /// Establish the common context and its parameter observations before querying it.
    pub(in crate::place) fn join(
        table: &ValueTable,
        parameters: &[usize],
        incoming: &[(&Self, &[usize])],
    ) -> Self {
        let Some((first, _)) = incoming.first() else {
            return Self::default();
        };
        let mut joined = (*first).clone();
        for (analysis, _) in &incoming[1..] {
            joined.context.retain_common(&analysis.context);
        }
        for (component, &parameter) in parameters.iter().enumerate() {
            joined.merge_parameter(
                table,
                parameter,
                incoming
                    .iter()
                    .map(|&(analysis, arguments)| (analysis, arguments[component])),
            );
        }
        joined
    }

    /// A scalar parameter keeps only logical bits proved by every incoming argument.
    /// Vector parameters still join normally, without acquiring scalar facts.
    fn merge_parameter<'a>(
        &mut self,
        table: &ValueTable,
        parameter: usize,
        incoming: impl Iterator<Item = (&'a Self, usize)>,
    ) {
        if !table[parameter].ty.is_scalar() {
            return;
        }
        let bits = incoming
            .map(|(facts, argument)| facts.bits(table, argument))
            .reduce(Bits::common)
            .unwrap_or_default()
            .restrict(table[parameter].ty.mask());
        if bits.mask != 0 {
            self.assume_bits(parameter, bits.mask, bits.value);
        }
    }
}

impl PathContext {
    fn retain_common(&mut self, other: &Self) {
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
    }
}
