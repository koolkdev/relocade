//! Path facts about logical bits, unsigned intervals and comparison outcomes.
//! Facts about a truncated value do not erase its other carrier bits.

use std::{cell::RefCell, collections::HashMap};

use crate::body::{ValueDefinition, ValueTable};

mod assume;
mod comparisons;
mod infer;
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
    known: HashMap<usize, Bits>,
    ranges: HashMap<usize, Range>,
    comparisons: Comparisons,
    computed: RefCell<HashMap<usize, Bits>>,
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
        if self.known.is_empty() {
            return None;
        }
        let bits = self.bits(table, id);
        let mask = table.values[id].ty.mask();
        (bits.mask & mask == mask).then_some(bits.value & mask)
    }
}
