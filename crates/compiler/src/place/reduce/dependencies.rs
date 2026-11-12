//! Track which reductions a changed fact can affect.

use super::{ScalarFacts, ValueTable};
use crate::place::facts::Comparison;
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Default)]
pub(in crate::place) struct ReductionGraph {
    users: Vec<Vec<usize>>,
    comparisons: FxHashMap<Comparison, Vec<usize>>,
    indexed: Vec<bool>,
    aliases: FxHashSet<(usize, usize)>,
    case_proofs: FxHashSet<usize>,
    pending: Vec<usize>,
    visited: Vec<u32>,
    revision: u32,
}

impl ReductionGraph {
    pub(super) fn changed(&mut self, previous: &ScalarFacts, facts: &ScalarFacts) -> Vec<usize> {
        facts.changed_values(previous, &mut self.pending);
        let mut comparison_changed = false;
        for comparison in facts.changed_comparisons(previous) {
            comparison_changed = true;
            if let Some(users) = self.comparisons.get(&comparison) {
                self.pending.extend(users.iter().copied());
            }
        }
        if self.pending.is_empty() && !comparison_changed {
            return Vec::new();
        }
        // Case splits can use constraints outside the value's input graph.
        self.pending.extend(self.case_proofs.iter().copied());
        self.revision = self.revision.wrapping_add(1);
        if self.revision == 0 {
            self.visited.fill(0);
            self.revision = 1;
        }
        let mut changed = Vec::new();
        while let Some(id) = self.pending.pop() {
            if id >= self.users.len() || self.visited[id] == self.revision {
                continue;
            }
            self.visited[id] = self.revision;
            changed.push(id);
            self.pending.extend(self.users[id].iter().copied());
        }
        changed
    }

    fn reserve(&mut self, count: usize) {
        if self.users.len() < count {
            self.users.resize_with(count, Vec::new);
            self.indexed.resize(count, false);
            self.visited.resize(count, 0);
        }
    }

    fn index(&mut self, values: &ValueTable, root: usize) {
        self.reserve(values.len());
        self.pending.push(root);
        while let Some(id) = self.pending.pop() {
            if self.indexed[id] {
                continue;
            }
            self.indexed[id] = true;
            if let Some(comparison) = Comparison::of(values, id) {
                self.comparisons.entry(comparison).or_default().push(id);
            }
            if let Some(result) = values.expression(id) {
                for &input in result.expression.inputs() {
                    self.users[input].push(id);
                    self.pending.push(input);
                }
            }
        }
    }

    pub(super) fn record(&mut self, values: &ValueTable, recipe: usize, residual: usize) {
        self.index(values, recipe);
        self.index(values, residual);
        if recipe != residual && self.aliases.insert((recipe, residual)) {
            // A fact about a lowered form also affects its original recipe.
            self.users[residual].push(recipe);
        }
    }

    pub(super) fn case_proof(&mut self, value: usize) {
        self.case_proofs.insert(value);
    }
}
