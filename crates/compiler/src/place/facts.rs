//! Path facts about logical bits, unsigned intervals and comparison outcomes.
//! Facts about a truncated value do not erase its other carrier bits.

use rustc_hash::FxHashMap;
use std::cell::RefCell;

use crate::{
    body::{ValueDefinition, ValueTable},
    integer::{low_mask, BinaryOp},
    Expression,
};

mod assume;
mod comparisons;
mod infer;
mod merge;
mod range;
use comparisons::Comparisons;
use range::Range;

#[derive(Clone, Copy, Default)]
struct Bits {
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
pub(super) struct Facts {
    // These sparse maps hash compiler-assigned value IDs, never guest values.
    known: FxHashMap<usize, Bits>,
    ranges: FxHashMap<usize, Range>,
    comparisons: Comparisons,
    computed: RefCell<FxHashMap<usize, Bits>>,
}

impl Clone for Facts {
    fn clone(&self) -> Self {
        Self {
            known: self.known.clone(),
            ranges: self.ranges.clone(),
            comparisons: self.comparisons.clone(),
            computed: RefCell::default(),
        }
    }
}

impl Facts {
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

    pub(super) fn constant(&self, table: &ValueTable, id: usize) -> Option<u64> {
        if let ValueDefinition::Constant(bits) = table[id].definition {
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

    /// An operand whose known bits make the other bitwise operand redundant.
    /// Unlike a logical constant, an identity must preserve the entire carrier.
    pub(super) fn bitwise_identity(&self, table: &ValueTable, id: usize) -> Option<usize> {
        let ValueDefinition::Expression(Expression::Binary {
            operator,
            left,
            right,
        }) = table[id].definition
        else {
            return None;
        };
        if !matches!(operator, BinaryOp::And | BinaryOp::Or) {
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
            BinaryOp::Or if mask & !right_zeros & !a.value == 0 => Some(left),
            BinaryOp::Or if mask & !left_zeros & !b.value == 0 => Some(right),
            BinaryOp::And if mask & !b.value & !left_zeros == 0 => Some(left),
            BinaryOp::And if mask & !a.value & !right_zeros == 0 => Some(right),
            _ => None,
        }
    }
}
