//! Analyze scalar encodings under an explicit path context.
//! F64 participates through its raw encoding; numeric range rules apply to integers.
//! Vectors are opaque to this analysis. Queries infer no vector facts, while
//! expression evaluation still folds complete vector literals.
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
mod context;
mod infer;
mod merge;
mod range;
mod scoped_map;
mod selects;
mod shifts;
pub(super) use context::{Assumption, ContextScope};
use context::{PathContext, SuspendedContext};
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

/// Derivation and its reusable worklist belong to this context's analysis owner.
/// Ordinary queries cannot change assumptions; entry and joins establish them first.
#[derive(Default)]
pub(super) struct ValueAnalysis {
    context: PathContext,
    derived: RefCell<DerivedFacts>,
    pending: RefCell<Vec<(usize, bool)>>,
    spare_results: DerivedFacts,
    suspended: Vec<SuspendedContext>,
}

#[derive(Default)]
struct DerivedFacts {
    bits: FxHashMap<usize, Bits>,
    select_constants: FxHashMap<usize, Option<u64>>,
}

impl DerivedFacts {
    /// Retain empty storage from finished contexts for the next child query.
    fn recycle(&mut self, mut finished: Self) {
        finished.bits.clear();
        finished.select_constants.clear();
        if finished.bits.capacity() > self.bits.capacity() {
            self.bits = finished.bits;
        }
        if finished.select_constants.capacity() > self.select_constants.capacity() {
            self.select_constants = finished.select_constants;
        }
    }
}

impl Clone for ValueAnalysis {
    /// A completed path can answer later join queries independently of active scopes.
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            ..Self::default()
        }
    }
}

impl ValueAnalysis {
    /// Whether known logical bits prove that these paths cannot coincide.
    pub(super) fn conflicts_with(&self, table: &ValueTable, other: &Self) -> bool {
        self.context
            .known
            .iter()
            .any(|(&id, &bits)| bits.conflicts(other.bits(table, id)))
    }

    fn assume_bits(&mut self, id: usize, mask: u64, value: u64) {
        let previous = self.context.known.get(&id).copied().unwrap_or_default();
        let bits = Bits {
            mask,
            value: value & mask,
        };
        if !previous.conflicts(bits) {
            self.record(id, previous.union(bits));
        }
    }

    fn record(&mut self, id: usize, bits: Bits) {
        self.context.known.insert(id, bits);
        self.invalidate_from(id);
    }

    fn invalidate_from(&mut self, id: usize) {
        // Only context construction changes observations. Calculations refer to
        // earlier values, so provisional answers below this ID remain valid.
        let derived = self.derived.get_mut();
        derived.bits.retain(|&input, _| input < id);
        derived.select_constants.retain(|&input, _| input < id);
    }

    /// Query constants under path assumptions; without them, use construction's folds.
    /// Scalar literals keep their carrier bits; inferred constants contain logical bits.
    /// Use ValueTable::carrier_bits to restore the carrier promised by construction.
    pub(super) fn constant(&self, table: &ValueTable, id: usize) -> Option<u64> {
        if !table[id].ty.is_scalar() {
            return None;
        }
        if let Some(bits) = table[id].scalar_literal() {
            return Some(bits);
        }
        if self.context.known.is_empty()
            && self.context.ranges.is_empty()
            && self.context.comparisons.is_empty()
        {
            return None;
        }
        if table.expression(id).is_none() {
            let mask = table[id].ty.mask();
            return self
                .context
                .known
                .get(&id)
                .and_then(|bits| (bits.mask & mask == mask).then_some(bits.value & mask));
        }
        self.inferred_constant(table, id)
    }

    /// Explicitly request structural inference, including without path assumptions.
    /// The bounded Boolean folding stage uses this for its candidate and cases.
    fn inferred_constant(&self, table: &ValueTable, id: usize) -> Option<u64> {
        if !table[id].ty.is_scalar() {
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
        if !table[id].ty.is_scalar() || !matches!(operator, BitwiseOp::And | BitwiseOp::Or) {
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
