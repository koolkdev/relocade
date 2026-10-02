//! Remember comparison outcomes independently of their result identities.
use std::collections::{hash_map::Entry, HashMap};

use crate::{body::ValueTable, integer::CompareOp};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Comparison {
    operator: CompareOp,
    left: usize,
    right: usize,
}

impl Comparison {
    fn canonical(
        table: &ValueTable,
        operator: CompareOp,
        left: usize,
        right: usize,
    ) -> (Self, bool) {
        let (operator, inverted) = match operator {
            CompareOp::Ne => (CompareOp::Eq, true),
            CompareOp::GeUnsigned => (CompareOp::LtUnsigned, true),
            CompareOp::GeSigned => (CompareOp::LtSigned, true),
            operator => (operator, false),
        };
        let mut left = table.representation(left);
        let mut right = table.representation(right);
        if operator == CompareOp::Eq && left > right {
            std::mem::swap(&mut left, &mut right);
        }
        (
            Self {
                operator,
                left,
                right,
            },
            inverted,
        )
    }
}

#[derive(Clone, Default)]
pub(super) struct Comparisons {
    outcomes: HashMap<Comparison, bool>,
}

impl Comparisons {
    /// Return the first possibly affected value for inference-cache invalidation.
    pub(super) fn assume(
        &mut self,
        table: &ValueTable,
        operator: CompareOp,
        left: usize,
        right: usize,
        truth: bool,
    ) -> Option<usize> {
        let (key, inverted) = Comparison::canonical(table, operator, left, right);
        if let Entry::Vacant(entry) = self.outcomes.entry(key) {
            entry.insert(truth ^ inverted);
            // The opposite predicate may have been constructed before this one.
            Some(key.left.min(key.right))
        } else {
            None
        }
    }

    pub(super) fn get(
        &self,
        table: &ValueTable,
        operator: CompareOp,
        left: usize,
        right: usize,
    ) -> Option<bool> {
        let (key, inverted) = Comparison::canonical(table, operator, left, right);
        self.outcomes.get(&key).map(|truth| truth ^ inverted)
    }
}
