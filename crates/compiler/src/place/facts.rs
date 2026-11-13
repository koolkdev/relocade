//! Path facts about scalar encodings up to 64 bits, integer ranges and comparisons.
//! F64 participates through its raw encoding; numeric range rules apply to integers.
//! Vector operations are opaque; overlapping scalar lane observations share
//! sparse word facts. Expression evaluation folds complete vector literals.
//! Facts about a truncated value do not erase its other carrier bits.

use crate::expression::BinaryOp as Binary;
use std::{cell::RefCell, num::NonZeroUsize, rc::Rc};

use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::low_mask,
    Expression,
};

mod assume;
mod comparisons;
mod constraints;
mod dependencies;
mod incremental;
mod infer;
mod lanes;
mod merge;
mod range;
mod selects;
mod shifts;
mod storage;
use comparisons::Comparisons;
use constraints::Constraints;
use dependencies::{Demand, Dependencies, WorkQueue};
pub(super) use infer::Inference;
use lanes::{LaneFacts, LaneObservation};
use range::Range;
use storage::Storage;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
struct Bits {
    // Known positions in a scalar encoding; zero means no knowledge.
    mask: u64,
    value: u64,
}

impl Bits {
    fn common(self, other: Self) -> Self {
        let mask = self.mask & other.mask & !(self.value ^ other.value);
        Self {
            mask,
            value: self.value & mask,
        }
    }

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

// Inferring bits and reducing a recipe are separate demands on the same value.
#[derive(Clone, Copy, Default)]
struct Answer {
    bits: Option<Bits>,
    // Offset IDs by one to keep the optional normal form in one word.
    residual: Option<NonZeroUsize>,
    revisit: bool,
    aliases: Option<NonZeroUsize>,
}

#[derive(Default)]
pub(super) struct ScalarFacts {
    // Sparse observations indexed by compiler-assigned value IDs.
    known: Storage<Bits>,
    lanes: Option<Box<LaneFacts>>,
    ranges: Storage<Range>,
    comparisons: Comparisons,
    constraints: Constraints,
    answers: RefCell<Storage<Answer>>,
    pending: Rc<RefCell<Vec<Inference>>>,
    updates: RefCell<WorkQueue>,
    dependencies: Rc<RefCell<Dependencies>>,
}

/// Observations established together before a block's facts are queried.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Assumption {
    Truth { condition: usize, truth: bool },
    Bits { value: usize, mask: u64, bits: u64 },
}

impl Clone for ScalarFacts {
    fn clone(&self) -> Self {
        Self {
            known: self.known.clone(),
            lanes: self.lanes.clone(),
            ranges: self.ranges.clone(),
            comparisons: self.comparisons.clone(),
            constraints: self.constraints.clone(),
            answers: self.answers.clone(),
            pending: self.pending.clone(),
            updates: self.updates.clone(),
            dependencies: self.dependencies.clone(),
        }
    }
}

impl ScalarFacts {
    pub(super) fn with_assumptions(
        mut self,
        table: &ValueTable,
        observations: impl IntoIterator<Item = Assumption>,
    ) -> Self {
        for observation in observations {
            match observation {
                Assumption::Truth { condition, truth } => self.assume(table, condition, truth),
                Assumption::Bits { value, mask, bits } => {
                    self.assume_bits(table, value, mask, bits)
                }
            }
        }
        self.propagate_constraints(table);
        self
    }

    fn has_observations(&self) -> bool {
        !self.known.is_empty()
            || self.lanes.is_some()
            || !self.ranges.is_empty()
            || !self.comparisons.is_empty()
    }

    /// Whether known logical bits prove that these paths cannot coincide.
    pub(super) fn conflicts_with(&self, table: &ValueTable, other: &Self) -> bool {
        self.known
            .any(|id, &bits| bits.conflicts(other.bits(table, id)))
            || self
                .lanes
                .as_deref()
                .is_some_and(|lanes| lanes.conflicts_with(table, other.lanes.as_deref()))
    }

    fn assume_bits(&mut self, table: &ValueTable, id: usize, mask: u64, value: u64) {
        let previous = self.known_bits(table, id);
        let bits = Bits {
            mask,
            value: value & mask,
        };
        if !previous.conflicts(bits) && bits.mask & !previous.mask != 0 {
            self.record(table, id, previous.union(bits));
        }
    }

    fn record(&mut self, table: &ValueTable, id: usize, bits: Bits) {
        let source = if let Some(observation) = LaneObservation::of(table, id) {
            observation.record(self.lanes.get_or_insert_with(Default::default), bits);
            // Another overlapping extraction may have been created before this
            // one. Its cached inference depends on the original vector value.
            observation.word.value
        } else {
            self.known.insert(id, bits);
            self.constraints.changed(id);
            self.queue(Demand::Bits(id));
            self.queue(Demand::Reduction(id));
            return;
        };
        self.changed(table, source);
    }

    pub(in crate::place) fn track_reduction(&self, table: &ValueTable, id: usize) {
        self.dependencies
            .borrow_mut()
            .track(table, Demand::Reduction(id));
    }

    fn changed(&mut self, table: &ValueTable, id: usize) {
        self.constraints.changed(id);
        self.refresh_bits(table);
        self.invalidate_from(id);
    }

    fn invalidate_from(&mut self, id: usize) {
        // These fact domains do not yet expose their precise dependencies.
        // Aliases can point to newer recipes, so clear both directions together.
        self.answers.get_mut().retain(|&input, answer| {
            answer.residual = None;
            answer.revisit = false;
            input < id
        });
        *self.updates.get_mut() = WorkQueue::default();
    }

    /// Scalar literals keep their carrier bits; inferred constants contain logical bits.
    /// Use ValueTable::carrier_bits to restore the carrier promised by construction.
    pub(super) fn constant(&self, table: &ValueTable, id: usize) -> Option<u64> {
        if !table[id].ty.is_scalar() {
            return None;
        }
        if let Some(bits) = table[id].scalar_literal() {
            return Some(bits);
        }
        // Construction already folded path-independent constants.
        if !self.has_observations() {
            return None;
        }
        self.inferred_constant(table, id)
    }

    /// Structural inference also applies without path assumptions during case proofs.
    fn inferred_constant(&self, table: &ValueTable, id: usize) -> Option<u64> {
        if !table[id].ty.is_scalar() {
            return None;
        }
        let bits = self.bits(table, id);
        let mask = table.values[id].ty.mask();
        (bits.mask & mask == mask).then_some(bits.value & mask)
    }

    fn bitwise_inputs(table: &ValueTable, id: usize) -> Option<(BitwiseOp, usize, usize)> {
        let ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Bitwise(operator),
            left,
            right,
        }) = table[id].definition
        else {
            return None;
        };
        if !table[id].ty.is_scalar() || !matches!(operator, BitwiseOp::And | BitwiseOp::Or) {
            return None;
        }
        Some((operator, left, right))
    }

    /// A scalar operand whose known bits make the other bitwise operand redundant.
    /// Unlike a logical constant, an identity must preserve the entire carrier.
    pub(super) fn bitwise_identity(&self, table: &ValueTable, id: usize) -> Option<usize> {
        let (operator, left, right) = Self::bitwise_inputs(table, id)?;
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
