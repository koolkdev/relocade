//! Shared reverse dependencies, stored as linked indices in one edge arena.

use super::{
    comparisons::Comparison,
    lanes::{LaneObservation, VectorWord},
    ScalarFacts,
};
use crate::{
    body::ValueTable,
    expression::{BinaryOp, CompareOp, UnaryOp},
    Expression,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::{cmp::Reverse, collections::BinaryHeap, num::NonZeroUsize};

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum Demand {
    Bits(usize),
    Reduction(usize),
    Constraint(usize),
}

impl Demand {
    fn index(self) -> (usize, usize) {
        match self {
            Self::Bits(id) => (id, 0),
            Self::Reduction(id) => (id, 1),
            Self::Constraint(id) => (id, 2),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Source {
    Bits(usize),
    Reduction(usize),
    Form(usize),
    Range(usize),
    Comparison(Comparison),
    Word(VectorWord),
}

#[derive(Clone, Copy, Default)]
struct Uses {
    heads: [Option<NonZeroUsize>; 4],
    indexed: u8,
}

struct Edge {
    user: Demand,
    next: Option<NonZeroUsize>,
}

#[derive(Default)]
pub(super) struct Dependencies {
    values: Vec<Uses>,
    edges: Vec<Edge>,
    aliases: FxHashSet<(usize, usize)>,
    proof_inputs: FxHashSet<(usize, usize)>,
    form_inputs: FxHashSet<(usize, usize)>,
    comparisons: FxHashMap<Comparison, Option<NonZeroUsize>>,
    words: FxHashMap<VectorWord, Option<NonZeroUsize>>,
}

/// Keep sparse pending work unique, without a bitmap copied at every path fork.
#[derive(Clone, Default)]
pub(super) struct WorkQueue {
    pending: BinaryHeap<Reverse<Demand>>,
    queued: FxHashSet<Demand>,
}

impl WorkQueue {
    pub(super) fn push(&mut self, demand: Demand) -> bool {
        if !self.queued.insert(demand) {
            return false;
        }
        self.pending.push(Reverse(demand));
        true
    }

    pub(super) fn next_bits(&mut self) -> Option<usize> {
        let Reverse(Demand::Bits(id)) = self.pending.peek()? else {
            return None;
        };
        let id = *id;
        self.pop();
        Some(id)
    }

    pub(super) fn pop(&mut self) -> Option<Demand> {
        let Reverse(demand) = self.pending.pop()?;
        self.queued.remove(&demand);
        Some(demand)
    }
}

impl Dependencies {
    pub(super) fn track(&mut self, table: &ValueTable, demand: Demand) {
        let (id, kind) = demand.index();
        self.values
            .resize(self.values.len().max(id + 1), Uses::default());
        if self.values[id].indexed & (1 << kind) != 0 {
            return;
        }
        self.values[id].indexed |= 1 << kind;
        let reduction = matches!(demand, Demand::Reduction(_));
        if reduction {
            // Missing inference still has consumers: a later fact will request
            // reduction directly until this value has an inferred answer.
            self.track(table, Demand::Bits(id));
            self.add(Source::Bits(id), demand);
        } else {
            self.track_facts(table, id, demand);
        }
        if let Some(result) = table.expression(id) {
            for &input in result.expression.inputs() {
                if !reduction || ScalarFacts::bitwise_inputs(table, id).is_some() {
                    self.add(Source::Bits(input), demand);
                }
                if reduction {
                    self.add(Source::Reduction(input), demand);
                }
            }
        }
    }

    pub(super) fn alias(&mut self, recipe: usize, residual: usize) {
        if recipe != residual && self.aliases.insert((recipe, residual)) {
            self.add(Source::Reduction(residual), Demand::Reduction(recipe));
        }
    }

    pub(super) fn form_input(&mut self, root: usize, input: usize) {
        if input != root && self.form_inputs.insert((root, input)) {
            self.add(Source::Form(input), Demand::Reduction(root));
        }
    }

    pub(super) fn proof_input(&mut self, table: &ValueTable, root: usize, input: usize) {
        if input != root && self.proof_inputs.insert((root, input)) {
            self.add(Source::Bits(input), Demand::Reduction(root));
            self.track_facts(table, input, Demand::Reduction(root));
        }
    }

    fn track_facts(&mut self, table: &ValueTable, id: usize, demand: Demand) {
        if let Some(observation) = LaneObservation::of(table, id) {
            self.add(Source::Word(observation.word), demand);
        }
        let Some(result) = table.expression(id) else {
            return;
        };
        match result.expression {
            Expression::Binary {
                operator: BinaryOp::Compare(CompareOp::Integer(operator)),
                left,
                right,
            } => {
                let (key, _) = Comparison::canonical(table, operator, left, right);
                self.add(Source::Comparison(key), demand);
                self.add(Source::Range(left), demand);
                self.add(Source::Range(right), demand);
            }
            Expression::Unary {
                operator: UnaryOp::ZeroTest { .. },
                input,
            } => {
                self.add(Source::Range(input), demand);
            }
            _ => {}
        }
    }

    fn add(&mut self, source: Source, user: Demand) {
        let head = match source {
            Source::Bits(id) | Source::Reduction(id) | Source::Range(id) | Source::Form(id) => {
                self.values
                    .resize(self.values.len().max(id + 1), Uses::default());
                let kind = match source {
                    Source::Bits(_) => 0,
                    Source::Reduction(_) => 1,
                    Source::Form(_) => 3,
                    _ => 2,
                };
                &mut self.values[id].heads[kind]
            }
            Source::Comparison(key) => self.comparisons.entry(key).or_default(),
            Source::Word(word) => self.words.entry(word).or_default(),
        };
        self.edges.push(Edge { user, next: *head });
        *head = NonZeroUsize::new(self.edges.len());
    }

    pub(super) fn users(&self, source: Source, mut visit: impl FnMut(Demand)) {
        let mut edge = match source {
            Source::Bits(id) => self.values.get(id).and_then(|uses| uses.heads[0]),
            Source::Reduction(id) => self.values.get(id).and_then(|uses| uses.heads[1]),
            Source::Range(id) => self.values.get(id).and_then(|uses| uses.heads[2]),
            Source::Form(id) => self.values.get(id).and_then(|uses| uses.heads[3]),
            Source::Comparison(key) => self.comparisons.get(&key).copied().flatten(),
            Source::Word(word) => self.words.get(&word).copied().flatten(),
        };
        while let Some(index) = edge {
            let user = &self.edges[index.get() - 1];
            visit(user.user);
            edge = user.next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Demand, WorkQueue};
    #[test]
    fn work_is_unique_until_completed_and_can_be_scheduled_again() {
        let mut work = WorkQueue::default();
        work.push(Demand::Bits(2));
        work.push(Demand::Bits(1));
        work.push(Demand::Bits(2));
        assert!(matches!(work.pop(), Some(Demand::Bits(1))));
        assert!(matches!(work.pop(), Some(Demand::Bits(2))));
        assert!(work.pop().is_none());
        work.push(Demand::Bits(1));
        assert!(matches!(work.pop(), Some(Demand::Bits(1))));
    }
}
