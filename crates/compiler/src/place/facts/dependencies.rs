//! Shared reverse dependencies, stored as linked indices in one edge arena.

use crate::body::ValueTable;
use rustc_hash::FxHashSet;
use std::{cmp::Reverse, collections::BinaryHeap, num::NonZeroUsize};

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum Demand {
    Bits(usize),
    Reduction(usize),
}

impl Demand {
    fn index(self) -> (usize, usize) {
        match self {
            Self::Bits(id) => (id, 0),
            Self::Reduction(id) => (id, 1),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Uses {
    heads: [Option<NonZeroUsize>; 2],
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
}

/// Keep sparse pending work unique, without a bitmap copied at every path fork.
#[derive(Clone, Default)]
pub(super) struct WorkQueue {
    pending: BinaryHeap<Reverse<Demand>>,
    queued: FxHashSet<Demand>,
}

impl WorkQueue {
    pub(super) fn push(&mut self, demand: Demand) {
        if self.queued.insert(demand) {
            self.pending.push(Reverse(demand));
        }
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
        if matches!(demand, Demand::Reduction(_)) {
            self.add(Demand::Bits(id), demand);
        }
        if let Some(result) = table.expression(id) {
            for &input in result.expression.inputs() {
                self.add(Demand::Bits(input), demand);
                if matches!(demand, Demand::Reduction(_)) {
                    self.add(Demand::Reduction(input), demand);
                }
            }
        }
    }

    pub(super) fn alias(&mut self, recipe: usize, residual: usize) {
        if recipe != residual && self.aliases.insert((recipe, residual)) {
            self.add(Demand::Reduction(residual), Demand::Reduction(recipe));
        }
    }

    pub(super) fn proof_input(&mut self, root: usize, input: usize) {
        if input != root && self.proof_inputs.insert((root, input)) {
            self.add(Demand::Bits(input), Demand::Reduction(root));
        }
    }

    fn add(&mut self, source: Demand, user: Demand) {
        let (id, kind) = source.index();
        self.values
            .resize(self.values.len().max(id + 1), Uses::default());
        let head = &mut self.values[id].heads[kind];
        self.edges.push(Edge { user, next: *head });
        *head = NonZeroUsize::new(self.edges.len());
    }

    pub(super) fn users(&self, source: Demand, mut visit: impl FnMut(Demand)) {
        let (id, kind) = source.index();
        let mut edge = self.values.get(id).and_then(|uses| uses.heads[kind]);
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
