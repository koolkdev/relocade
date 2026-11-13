//! Remember comparison outcomes independently of their result identities.
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

use crate::{body::ValueTable, integer::CompareOp};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(in crate::place) struct Comparison {
    operator: CompareOp,
    left: usize,
    right: usize,
}

impl Comparison {
    pub(in crate::place) fn of(table: &ValueTable, value: usize) -> Option<Self> {
        use crate::expression::{BinaryOp, CompareOp as ComparisonOp};
        let crate::Expression::Binary {
            operator: BinaryOp::Compare(ComparisonOp::Integer(operator)),
            left,
            right,
        } = table.expression(value)?.expression
        else {
            return None;
        };
        Some(Self::canonical(table, operator, left, right).0)
    }

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
    outcomes: FxHashMap<Comparison, bool>,
}

impl Comparisons {
    pub(super) fn changed<'a>(
        &'a self,
        previous: &'a Self,
    ) -> impl Iterator<Item = Comparison> + 'a {
        super::changed_keys(&self.outcomes, &previous.outcomes).copied()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    pub(super) fn retain_common(&mut self, other: &Self) {
        self.outcomes
            .retain(|key, truth| other.outcomes.get(key) == Some(truth));
    }

    /// Return a lower bound for values whose cached inference may change.
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
            // An equivalent or opposite predicate may precede this one, but
            // must follow both canonical operands, including through conversions.
            Some(key.left.max(key.right))
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
