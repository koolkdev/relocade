//! Path facts about scalar encodings up to 64 bits, integer ranges and comparisons.
//! F64 participates through its raw encoding; numeric range rules apply to integers.
//! Facts about a truncated value do not erase its other carrier bits.

use rustc_hash::FxHashMap;
use std::cell::RefCell;

use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::low_mask,
    Expression,
};

mod assume;
mod comparisons;
mod infer;
mod merge;
mod range;
mod scoped_map;
use comparisons::Comparisons;
use range::Range;
use scoped_map::ScopedMap;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct Bits {
    // Known positions in a scalar encoding; zero means no knowledge.
    mask: u64,
    value: u64,
}

impl Bits {
    fn conflicts(self, other: Self) -> bool {
        (self.value ^ other.value) & self.mask & other.mask != 0
    }

    fn union(self, other: Self) -> Self {
        Self {
            mask: self.mask | other.mask,
            value: self.value | other.value,
        }
    }

    fn restrict(self, mask: u64) -> Self {
        Self {
            mask: self.mask & mask,
            value: self.value & mask,
        }
    }
}

#[derive(Default)]
pub(super) struct ScalarFacts {
    // These sparse maps hash compiler-assigned value IDs, never guest values.
    known: ScopedMap<usize, Bits>,
    ranges: ScopedMap<usize, Range>,
    comparisons: Comparisons,
    computed: RefCell<FxHashMap<usize, Bits>>,
}

pub(super) struct Checkpoint {
    known: scoped_map::Checkpoint,
    ranges: scoped_map::Checkpoint,
    comparisons: scoped_map::Checkpoint,
}

impl Clone for ScalarFacts {
    fn clone(&self) -> Self {
        Self {
            known: self.known.clone(),
            ranges: self.ranges.clone(),
            comparisons: self.comparisons.clone(),
            computed: RefCell::default(),
        }
    }
}

impl ScalarFacts {
    /// Save a nested path scope without copying its inherited facts.
    pub(super) fn checkpoint(&mut self) -> Checkpoint {
        Checkpoint {
            known: self.known.checkpoint(),
            ranges: self.ranges.checkpoint(),
            comparisons: self.comparisons.checkpoint(),
        }
    }

    /// Restore the most recent scope and discard inferences from its changed facts.
    pub(super) fn restore(&mut self, checkpoint: Checkpoint) {
        self.known.restore(checkpoint.known);
        self.ranges.restore(checkpoint.ranges);
        self.comparisons.restore(checkpoint.comparisons);
        self.computed = RefCell::default();
    }

    /// Whether known logical bits prove that these paths cannot coincide.
    pub(super) fn conflicts_with(&self, table: &ValueTable, other: &Self) -> bool {
        self.known
            .iter()
            .any(|(&id, &bits)| bits.conflicts(other.bits(table, id)))
    }

    pub(super) fn assume_bits(&mut self, id: usize, mask: u64, value: u64) {
        let previous = self.known.get(&id).copied().unwrap_or_default();
        let bits = Bits {
            mask,
            value: value & mask,
        };
        if !previous.conflicts(bits) {
            self.record(id, previous.union(bits));
        }
    }

    fn record(&mut self, id: usize, bits: Bits) {
        self.known.insert(id, bits);
        self.invalidate_from(id);
    }

    fn invalidate_from(&mut self, id: usize) {
        // Calculations refer only to earlier values. Their cached inputs remain
        // valid when learning a fact about this value and its possible users.
        self.computed.get_mut().retain(|&input, _| input < id);
    }

    /// Scalar literals keep their carrier bits; inferred constants contain logical bits.
    /// Use ValueTable::carrier_bits to restore the carrier promised by construction.
    pub(super) fn constant(&self, table: &ValueTable, id: usize) -> Option<u64> {
        if let Some(bits) = table[id].scalar_literal() {
            return Some(bits);
        }
        // Construction already folded path-independent constants.
        if self.known.is_empty() && self.ranges.is_empty() && self.comparisons.is_empty() {
            return None;
        }
        let bits = self.bits(table, id);
        let mask = table.values[id].ty.mask();
        (bits.mask & mask == mask).then_some(bits.value & mask)
    }

    /// A scalar operand whose known bits make the other bitwise operand redundant.
    /// Unlike a logical constant, an identity must preserve the entire carrier.
    pub(super) fn bitwise_identity(&self, table: &ValueTable, id: usize) -> Option<usize> {
        let ValueDefinition::Expression(Expression::Bitwise {
            operator,
            left,
            right,
        }) = table[id].definition
        else {
            return None;
        };
        if !matches!(operator, BitwiseOp::And | BitwiseOp::Or) {
            return None;
        }
        let mask = table[id].ty.carrier().mask();
        let a = self.bits(table, left);
        let b = self.bits(table, right);
        // Logical facts leave upper carrier bits unknown. Only physical bounds
        // can prove those bits zero; a narrow logical type cannot.
        let left_zeros = (a.mask & !a.value) | (mask & !low_mask(table.bounds[left].unsigned));
        let right_zeros = (b.mask & !b.value) | (mask & !low_mask(table.bounds[right].unsigned));
        match operator {
            BitwiseOp::Or if mask & !right_zeros & !a.value == 0 => Some(left),
            BitwiseOp::Or if mask & !left_zeros & !b.value == 0 => Some(right),
            BitwiseOp::And if mask & !b.value & !left_zeros == 0 => Some(left),
            BitwiseOp::And if mask & !a.value & !right_zeros == 0 => Some(right),
            _ => None,
        }
    }
}
