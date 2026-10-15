//! Share scalar observations of overlapping vector lanes in 64-bit words.
//!
//! Vector operations remain opaque. The path's ordinary scalar inference cache
//! queries this sparse storage; it has no separate worklist or cache lifecycle.

use super::{Bits, ScalarFacts};
use crate::{
    body::{ValueDefinition, ValueTable},
    expression::UnaryOp as Unary,
    Expression, Type,
};
use rustc_hash::FxHashMap;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct VectorWord {
    pub(super) value: usize,
    half: u8,
}

impl VectorWord {
    fn new(table: &ValueTable, value: usize, half: u8) -> Self {
        Self {
            value: table.representation(value),
            half,
        }
    }

    fn bits(self, table: &ValueTable, facts: Option<&LaneFacts>) -> Bits {
        if let ValueDefinition::Literal(literal) = table[self.value].definition {
            return Bits {
                mask: u64::MAX,
                value: (u128::from(literal) >> (self.half * 64)) as u64,
            };
        }
        facts
            .and_then(|facts| facts.words.get(&self).copied())
            .unwrap_or_default()
    }
}

pub(super) struct LaneObservation {
    pub(super) word: VectorWord,
    offset: u8,
    mask: u64,
}

impl LaneObservation {
    pub(super) fn of(table: &ValueTable, id: usize) -> Option<Self> {
        let value = table[table.representation(id)];
        let ValueDefinition::Expression(Expression::Unary { operator, input }) = value.definition
        else {
            return None;
        };
        let lane = match operator {
            Unary::ExtractLane { lane } => lane,
            Unary::Reinterpret
                if value.ty == Type::I64 && table[input].ty.carrier() == Type::V128 =>
            {
                0
            }
            _ => return None,
        };
        let start = lane * value.ty.bits();
        Some(Self {
            word: VectorWord::new(table, input, start / 64),
            offset: start % 64,
            mask: value.ty.mask(),
        })
    }

    fn read(&self, table: &ValueTable, facts: &LaneFacts) -> Bits {
        let bits = self.word.bits(table, Some(facts));
        Bits {
            mask: bits.mask >> self.offset,
            value: bits.value >> self.offset,
        }
        .restrict(self.mask)
    }

    pub(super) fn record(&self, facts: &mut LaneFacts, bits: Bits) {
        let bits = bits.restrict(self.mask);
        facts.record(
            self.word,
            Bits {
                mask: bits.mask << self.offset,
                value: bits.value << self.offset,
            },
        );
    }
}

#[derive(Clone, Default)]
pub(super) struct LaneFacts {
    words: FxHashMap<VectorWord, Bits>,
}

impl LaneFacts {
    fn record(&mut self, word: VectorWord, bits: Bits) {
        if bits.mask == 0 {
            return;
        }
        let previous = self.words.entry(word).or_default();
        if !previous.conflicts(bits) {
            *previous = previous.union(bits);
        }
    }

    pub(super) fn conflicts_with(&self, table: &ValueTable, other: Option<&Self>) -> bool {
        self.words
            .iter()
            .any(|(&word, &bits)| bits.conflicts(word.bits(table, other)))
    }

    pub(super) fn retain_common(&mut self, other: &Self) -> bool {
        self.words.retain(|word, bits| {
            *bits = bits.common(other.words.get(word).copied().unwrap_or_default());
            bits.mask != 0
        });
        !self.words.is_empty()
    }
}

impl ScalarFacts {
    /// Recognized lane observations are stored only in their canonical word.
    /// Other scalar values continue to use the existing scalar assumption map.
    pub(super) fn known_bits(&self, table: &ValueTable, id: usize) -> Bits {
        let bits = self.known.get(&id).copied().unwrap_or_default();
        if let Some(lanes) = self.lanes.as_deref() {
            if let Some(observation) = LaneObservation::of(table, id) {
                return bits
                    .union(observation.read(table, lanes))
                    .restrict(table[id].ty.mask());
            }
        }
        bits
    }

    pub(super) fn merge_vector_parameter<'a>(
        &mut self,
        table: &ValueTable,
        parameter: usize,
        incoming: impl Iterator<Item = (&'a Self, usize)> + Clone,
    ) {
        let mut recorded = false;
        for half in 0..table[parameter].ty.bits() / 64 {
            let bits = incoming
                .clone()
                .map(|(facts, argument)| {
                    VectorWord::new(table, argument, half).bits(table, facts.lanes.as_deref())
                })
                .reduce(Bits::common)
                .unwrap_or_default();
            if bits.mask != 0 {
                self.lanes
                    .get_or_insert_with(Default::default)
                    .record(VectorWord::new(table, parameter, half), bits);
                recorded = true;
            }
        }
        // A V64 parameter receives only its logical low word; the upper carrier
        // half can differ between incoming values and stays unknown.
        if recorded {
            self.changed(parameter);
        }
    }
}
