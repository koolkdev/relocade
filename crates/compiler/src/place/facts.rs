//! Known bits of scalar and vector encodings, with scalar ranges and comparisons.
//! F64 participates through its raw encoding; numeric range rules apply to integers.
//! Facts about a truncated value do not erase its other carrier bits.

use rustc_hash::FxHashMap;
use std::cell::RefCell;

use crate::{
    body::{ValueDefinition, ValueTable},
    literal::Literal,
};

mod assume;
mod bits;
mod comparisons;
mod encoding;
mod identity;
mod infer;
mod merge;
mod range;
mod scoped_map;
use bits::KnownBits;
use comparisons::Comparisons;
use range::Range;
use scoped_map::ScopedMap;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct ValueFacts {
    // These sparse maps hash compiler-assigned value IDs, never guest values.
    known: ScopedMap<usize, KnownBits>,
    ranges: ScopedMap<usize, Range>,
    comparisons: Comparisons,
    computed: RefCell<FxHashMap<usize, KnownBits>>,
}

pub(super) struct Checkpoint {
    known: scoped_map::Checkpoint,
    ranges: scoped_map::Checkpoint,
    comparisons: scoped_map::Checkpoint,
}

impl Clone for ValueFacts {
    fn clone(&self) -> Self {
        Self {
            known: self.known.clone(),
            ranges: self.ranges.clone(),
            comparisons: self.comparisons.clone(),
            computed: RefCell::default(),
        }
    }
}

impl ValueFacts {
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

    pub(super) fn assume_bits(&mut self, id: usize, mask: u128, value: u128) {
        let previous = self.known.get(&id).copied().unwrap_or_default();
        let bits = KnownBits {
            mask,
            value: value & mask,
        };
        if !previous.conflicts(bits) {
            self.record(id, previous.union(bits));
        }
    }

    fn record(&mut self, id: usize, bits: KnownBits) {
        self.known.insert(id, bits);
        self.invalidate_from(id);
    }

    fn invalidate_from(&mut self, id: usize) {
        // Calculations refer only to earlier values. Their cached inputs remain
        // valid when learning a fact about this value and its possible users.
        self.computed.get_mut().retain(|&input, _| input < id);
    }

    /// Complete constant encodings, restoring scalar carriers from logical facts.
    pub(super) fn constant(&self, table: &ValueTable, id: usize) -> Option<Literal> {
        if let ValueDefinition::Literal(bits) = table[id].definition {
            return Some(bits);
        }
        // Construction folds path-independent scalar constants. Lane extracts
        // can additionally become constant from a partially known vector.
        if table[id].ty.is_scalar()
            && !matches!(
                table[id].definition,
                ValueDefinition::Expression(crate::Expression::VectorExtract { .. })
            )
            && self.known.is_empty()
            && self.ranges.is_empty()
            && self.comparisons.is_empty()
        {
            return None;
        }
        let bits = self.bits(table, id);
        let ty = table[id].ty;
        let mask = ty.bit_mask();
        if bits.mask & mask != mask {
            return None;
        }
        Some(if ty.is_scalar() {
            table.carrier_bits(id, bits.value as u64).into()
        } else {
            bits.value.into()
        })
    }
}
