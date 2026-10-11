//! Analyze encoding bits under an explicit path context.
//! F64 participates through its raw encoding; numeric range rules apply to integers.
//! Scalar facts use u64 storage. Vector facts allocate separate u128 storage on demand.
//! Facts about a truncated value do not erase its other carrier bits.

use crate::expression::UnaryOp as Unary;
use rustc_hash::FxHashMap;
use std::cell::RefCell;

use crate::{
    body::{ValueDefinition, ValueTable},
    Expression,
};

mod assume;
mod bits;
mod comparisons;
mod context;
mod identity;
mod infer;
mod merge;
mod paths;
mod range;
mod scoped_map;
mod selects;
mod shifts;
use bits::Bits;
pub(super) use context::Assumption;
use context::{PathContext, SuspendedContext};
pub(super) use paths::PathSnapshot;
use range::Range;
use scoped_map::ScopedMap;

#[cfg(test)]
mod tests;

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
    vectors: Option<Box<FxHashMap<usize, Bits<u128>>>>,
    select_constants: FxHashMap<usize, Option<u64>>,
    path_exclusions: Option<Box<FxHashMap<usize, bool>>>,
}

impl DerivedFacts {
    /// Retain empty storage from finished contexts for the next child query.
    fn recycle(&mut self, mut finished: Self) {
        finished.bits.clear();
        finished.select_constants.clear();
        if let Some(mut vectors) = finished.vectors {
            vectors.clear();
            if self
                .vectors
                .as_ref()
                .is_none_or(|spare| vectors.capacity() > spare.capacity())
            {
                self.vectors = Some(vectors);
            }
        }
        if finished.bits.capacity() > self.bits.capacity() {
            self.bits = finished.bits;
        }
        if finished.select_constants.capacity() > self.select_constants.capacity() {
            self.select_constants = finished.select_constants;
        }
        if let Some(mut paths) = finished.path_exclusions {
            paths.clear();
            if self
                .path_exclusions
                .as_ref()
                .is_none_or(|spare| paths.capacity() > spare.capacity())
            {
                self.path_exclusions = Some(paths);
            }
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
    fn conflicts_with(&self, table: &ValueTable, other: &Self) -> bool {
        self.context
            .known
            .iter()
            .any(|(&id, &bits)| bits.conflicts(other.bits(table, id)))
            || self.context.vector_known.as_ref().is_some_and(|known| {
                known
                    .iter()
                    .any(|(&id, &bits)| bits.conflicts(other.vector_bits(table, id)))
            })
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
        if let Some(vectors) = &mut derived.vectors {
            vectors.retain(|&input, _| input < id);
        }
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
            && self
                .context
                .vector_known
                .as_ref()
                .is_none_or(|known| known.is_empty())
            && !matches!(
                table[id].definition,
                ValueDefinition::Expression(Expression::Unary {
                    operator: Unary::ExtractLane { .. },
                    ..
                })
            )
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
    fn record_vector(&mut self, id: usize, bits: Bits<u128>) {
        self.context
            .vector_known
            .get_or_insert_with(Box::default)
            .insert(id, bits);
        self.invalidate_from(id);
    }

    pub(in crate::place) fn vector_constant(&self, table: &ValueTable, id: usize) -> Option<u128> {
        let bits = self.vector_bits(table, id);
        (bits.mask == u128::MAX).then_some(bits.value)
    }

    /// Bits outside the observation may take any value in this replacement.
    /// Such a constant must not become a complete semantic equivalence.
    pub(in crate::place) fn observed_constant(
        &self,
        table: &ValueTable,
        id: usize,
        observed: u128,
    ) -> Option<u128> {
        if observed == 0 {
            return Some(0);
        }
        if table[id].ty.is_scalar() {
            let bits = self.scalar_carrier_bits(table, id);
            let mask = observed as u64 & table[id].ty.carrier().mask();
            (bits.mask & mask == mask).then_some(u128::from(bits.value & mask))
        } else {
            let bits = self.vector_bits(table, id);
            (bits.mask & observed == observed).then_some(bits.value & observed)
        }
    }
}
