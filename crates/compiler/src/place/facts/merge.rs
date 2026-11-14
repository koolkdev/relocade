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
            joined.retain_common(table, facts);
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

    fn retain_common(&mut self, table: &ValueTable, other: &Self) {
        self.refresh_bits(table);
        other.refresh_bits(table);
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
        self.answers
            .borrow_mut()
            .intersect(&other.answers.borrow(), |a, b| {
                let mut answer = *a;
                answer.bits = a.bits.zip(b.bits).map(|(a, b)| a.common(b));
                answer.bits_complete = a.bits_complete && b.bits_complete && a.bits == b.bits;
                if a.residual != b.residual {
                    answer.residual = None;
                }
                answer.revisit |= b.revisit;
                answer.literal &= b.literal && answer.residual.is_some();
                if a.form != b.form {
                    answer.form = None;
                    answer.revisit = true;
                }
                if a.aliases != b.aliases {
                    answer.aliases = None;
                }
                (answer.bits.is_some() || answer.residual.is_some() || answer.aliases.is_some())
                    .then_some(answer)
            });
        *self.updates.borrow_mut() = Default::default();
    }
}
