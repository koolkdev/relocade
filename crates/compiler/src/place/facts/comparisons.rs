//! Remember comparison outcomes independently of their result identities.
use super::storage::Storage;
use rustc_hash::FxHashMap;
use std::{cell::RefCell, rc::Rc};

use crate::{body::ValueTable, integer::CompareOp};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Comparison {
    operator: CompareOp,
    left: usize,
    right: usize,
}

impl Comparison {
    pub(super) fn canonical(
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

#[derive(Default)]
struct Keys {
    ids: FxHashMap<Comparison, usize>,
    values: Vec<Comparison>,
}

#[derive(Clone, Default)]
pub(super) struct Comparisons {
    keys: Rc<RefCell<Keys>>,
    outcomes: Storage<bool>,
}

impl Comparisons {
    pub(super) fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    pub(super) fn retain_common(&mut self, other: &Self) {
        let keys = self.keys.borrow();
        self.outcomes
            .retain(|id, truth| other.outcome(keys.values[*id]) == Some(*truth));
    }

    /// Return the canonical predicate when this path learns a new outcome.
    pub(super) fn assume(
        &mut self,
        table: &ValueTable,
        operator: CompareOp,
        left: usize,
        right: usize,
        truth: bool,
    ) -> Option<Comparison> {
        let (key, inverted) = Comparison::canonical(table, operator, left, right);
        let mut keys = self.keys.borrow_mut();
        let id = if let Some(&id) = keys.ids.get(&key) {
            id
        } else {
            let id = keys.values.len();
            keys.values.push(key);
            keys.ids.insert(key, id);
            id
        };
        if self.outcomes.contains_key(&id) {
            return None;
        }
        self.outcomes.insert(id, truth ^ inverted);
        Some(key)
    }

    fn outcome(&self, key: Comparison) -> Option<bool> {
        let id = *self.keys.borrow().ids.get(&key)?;
        self.outcomes.get(&id).copied()
    }

    pub(super) fn get(
        &self,
        table: &ValueTable,
        operator: CompareOp,
        left: usize,
        right: usize,
    ) -> Option<bool> {
        let (key, inverted) = Comparison::canonical(table, operator, left, right);
        self.outcome(key).map(|truth| truth ^ inverted)
    }
}
