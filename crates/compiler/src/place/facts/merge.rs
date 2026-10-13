//! Keep only knowledge that holds on every incoming path.

use super::{Bits, Range, ScalarFacts};
use crate::body::ValueTable;

#[cfg(test)]
mod tests;

impl ScalarFacts {
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
        for (facts, _) in &incoming[1..] {
            joined.retain_common(facts);
        }
        for (component, &parameter) in parameters.iter().enumerate() {
            joined.merge_parameter(
                table,
                parameter,
                incoming
                    .iter()
                    .map(|&(facts, arguments)| (facts, arguments[component])),
            );
        }
        joined.propagate_constraints(table);
        joined
    }

    /// Parameters keep only logical bits proved by every incoming argument.
    fn merge_parameter<'a>(
        &mut self,
        table: &ValueTable,
        parameter: usize,
        incoming: impl Iterator<Item = (&'a Self, usize)> + Clone,
    ) {
        if !table[parameter].ty.is_scalar() {
            self.merge_vector_parameter(table, parameter, incoming);
            return;
        }
        let bits = incoming
            .map(|(facts, argument)| facts.bits(table, argument))
            .reduce(Bits::common)
            .unwrap_or_default()
            .restrict(table[parameter].ty.mask());
        if bits.mask != 0 {
            self.assume_bits(table, parameter, bits.mask, bits.value);
        }
    }

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
        self.constraints.retain_known(&self.known);
        if !self
            .lanes
            .as_mut()
            .zip(other.lanes.as_deref())
            .is_some_and(|(lanes, other)| lanes.retain_common(other))
        {
            self.lanes = None;
        }
        // Earlier inferences may depend on knowledge just discarded.
        self.computed.get_mut().clear();
    }
}
