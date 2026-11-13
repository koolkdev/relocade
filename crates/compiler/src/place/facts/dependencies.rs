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
    pending: WorkQueue,
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

    fn add(&mut self, source: Demand, user: Demand) {
        let (id, kind) = source.index();
        self.values
            .resize(self.values.len().max(id + 1), Uses::default());
        let head = &mut self.values[id].heads[kind];
        self.edges.push(Edge { user, next: *head });
        *head = NonZeroUsize::new(self.edges.len());
    }

    pub(super) fn invalidate(&mut self, id: usize, mut invalidate: impl FnMut(Demand) -> bool) {
        self.pending.push(Demand::Bits(id));
        self.pending.push(Demand::Reduction(id));
        while let Some(demand) = self.pending.pop() {
            if !invalidate(demand) {
                continue;
            }
            let (id, kind) = demand.index();
            let mut edge = self.values.get(id).and_then(|uses| uses.heads[kind]);
            while let Some(index) = edge {
                let user = &self.edges[index.get() - 1];
                self.pending.push(user.user);
                edge = user.next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Demand, Dependencies};

    #[test]
    fn converging_and_alias_dependencies_visit_each_cached_answer_once() {
        let mut dependencies = Dependencies::default();
        dependencies.add(Demand::Bits(0), Demand::Bits(1));
        dependencies.add(Demand::Bits(0), Demand::Bits(2));
        dependencies.add(Demand::Bits(1), Demand::Bits(2));
        dependencies.add(Demand::Bits(2), Demand::Reduction(2));
        dependencies.alias(3, 2);
        dependencies.alias(3, 2);
        let mut demanded = rustc_hash::FxHashSet::from_iter([
            Demand::Bits(0),
            Demand::Bits(1),
            Demand::Bits(2),
            Demand::Reduction(2),
            Demand::Reduction(3),
            Demand::Bits(9),
        ]);
        dependencies.invalidate(0, |demand| demanded.remove(&demand));
        assert_eq!(demanded.len(), 1);
        assert!(demanded.contains(&Demand::Bits(9)));
    }
}
