//! Own the live refinements, their branch snapshots, and notifications at joins.
use super::*;
use crate::ssa::versions::{Snapshot, Versions};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct PredicateId(usize);

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FactKey {
    Value(usize),
    Predicate(PredicateId),
}

impl FactKey {
    fn order(self) -> (usize, bool) {
        match self {
            Self::Value(value) => (value, false),
            Self::Predicate(PredicateId(index)) => (index, true),
        }
    }
}

enum Undo {
    Value {
        root: usize,
        previous: Option<usize>,
    },
    Predicate {
        id: PredicateId,
        previous: Option<bool>,
    },
}

pub(super) struct Scopes<'a> {
    dominators: &'a Dominators,
    roots: Vec<usize>,
    renamed: Vec<Option<usize>>,
    undo: Vec<Undo>,
    versions: Versions,
    outgoing: Vec<Snapshot>,
    current: Snapshot,
    block: BlockId,
    complete: Vec<bool>,
    frontiers: Vec<Vec<usize>>,
    merges: Vec<Vec<FactKey>>,
    predicate_ids: FxHashMap<Predicate, PredicateId>,
    predicate_keys: Vec<Predicate>,
    predicates: Vec<Option<bool>>,
}

impl<'a> Scopes<'a> {
    pub(super) fn new(dominators: &'a Dominators, incoming: &[Vec<usize>], values: usize) -> Self {
        Self {
            dominators,
            roots: (0..values).collect(),
            renamed: vec![None; values],
            undo: Vec::new(),
            versions: Versions::new(),
            outgoing: vec![Snapshot::default(); incoming.len()],
            current: Snapshot::default(),
            block: BlockId(0),
            complete: vec![false; incoming.len()],
            frontiers: dominators.frontiers(incoming),
            merges: vec![Vec::new(); incoming.len()],
            predicate_ids: FxHashMap::default(),
            predicate_keys: Vec::new(),
            predicates: Vec::new(),
        }
    }

    pub(super) fn extend(&mut self, values: usize) {
        self.roots.extend(self.roots.len()..values);
        self.renamed.resize(values, None);
    }

    pub(super) fn block(&self) -> BlockId {
        self.block
    }
    pub(super) fn root(&self, value: usize) -> usize {
        self.roots[value]
    }
    pub(super) fn refinement(&self, value: usize) -> Option<usize> {
        self.renamed[self.root(value)]
    }
    pub(super) fn complete(&self, block: usize) -> bool {
        self.complete[block]
    }
    pub(super) fn discard(&mut self, block: usize) {
        self.complete[block] = true;
    }

    pub(super) fn enter(&mut self, block: BlockId) -> usize {
        self.block = block;
        self.current = self.dominators.parent[block.0]
            .map_or(Snapshot::default(), |parent| self.outgoing[parent]);
        self.undo.len()
    }

    pub(super) fn finish(&mut self) {
        self.outgoing[self.block.0] = self.current;
        self.complete[self.block.0] = true;
    }

    pub(super) fn leave(&mut self, checkpoint: usize) {
        while self.undo.len() > checkpoint {
            match self.undo.pop().unwrap() {
                Undo::Value { root, previous } => self.renamed[root] = previous,
                Undo::Predicate { id, previous } => self.predicates[id.0] = previous,
            }
        }
    }

    pub(super) fn refine(&mut self, graph: &mut SsaGraph, source: usize) -> usize {
        let refined = graph.values.push(Value {
            ty: graph.values[source].ty,
            definition: ValueDefinition::RefinedAlias {
                source,
                block: self.block,
            },
        });
        self.extend(graph.values.len());
        let root = self.root(source);
        self.roots[refined] = root;
        let previous = self.renamed[root].replace(refined);
        self.undo.push(Undo::Value { root, previous });
        self.current = self
            .versions
            .with_value(self.current, self.block, root, refined);
        for &join in &self.frontiers[self.block.0] {
            if !self.complete[join]
                && graph
                    .values
                    .definition_block(root)
                    .is_some_and(|owner| self.dominators.dominates(owner.0, join))
            {
                self.merges[join].push(FactKey::Value(root));
            }
        }
        refined
    }

    pub(super) fn predicate_id(&mut self, key: Predicate) -> PredicateId {
        if let Some(&id) = self.predicate_ids.get(&key) {
            return id;
        }
        let id = PredicateId(self.predicates.len());
        self.predicate_ids.insert(key, id);
        self.predicate_keys.push(key);
        self.predicates.push(None);
        id
    }

    pub(super) fn predicate(&self, id: PredicateId) -> Option<bool> {
        self.predicates[id.0]
    }

    pub(super) fn assume_predicate(&mut self, graph: &SsaGraph, id: PredicateId, truth: bool) {
        if self.predicates[id.0].is_some() {
            return;
        }
        let previous = self.predicates[id.0].replace(truth);
        self.undo.push(Undo::Predicate { id, previous });
        self.current = self
            .versions
            .with_truth(self.current, self.block, id.0, truth);
        for &join in &self.frontiers[self.block.0] {
            if !self.complete[join]
                && self.predicate_keys[id.0].inputs().into_iter().all(|input| {
                    graph
                        .values
                        .definition_block(input)
                        .is_none_or(|owner| self.dominators.dominates(owner.0, join))
                })
            {
                self.merges[join].push(FactKey::Predicate(id));
            }
        }
    }

    pub(super) fn pending(&mut self) -> Vec<FactKey> {
        let mut keys = std::mem::take(&mut self.merges[self.block.0]);
        keys.sort_unstable_by_key(|key| key.order());
        keys.dedup();
        keys
    }

    pub(super) fn value_at(&self, source: usize, root: usize, fallback: usize) -> usize {
        self.versions
            .value(self.outgoing[source], root)
            .unwrap_or(fallback)
    }

    pub(super) fn predicate_at(&self, source: usize, id: PredicateId) -> Option<bool> {
        self.versions
            .truth(self.outgoing[source], id.0)
            .or(self.predicate(id))
    }
}

#[cfg(test)]
mod tests;
