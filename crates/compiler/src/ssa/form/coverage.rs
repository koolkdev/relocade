//! Prove shared execution from direct uses in a fixed control graph.
use super::*;
use rustc_hash::FxHashMap;

pub(super) struct Coverage<'a> {
    pub(super) dominators: &'a Dominators,
    predecessors: Vec<Vec<usize>>,
    successors: Vec<usize>,
    marks: Vec<usize>,
    remaining: Vec<usize>,
    generation: usize,
    cache: FxHashMap<Vec<BlockId>, bool>,
}

impl<'a> Coverage<'a> {
    pub(super) fn new(
        graph: &crate::body::FunctionGraph,
        reachable: &[bool],
        dominators: &'a Dominators,
    ) -> Self {
        let mut successors = super::super::successors(graph, reachable);
        for targets in &mut successors {
            targets.sort_unstable();
            targets.dedup();
        }
        let mut predecessors = vec![Vec::new(); successors.len()];
        for (source, targets) in successors.iter().enumerate() {
            for &target in targets {
                predecessors[target].push(source);
            }
        }
        let count = successors.len();
        Self {
            dominators,
            predecessors,
            successors: successors.iter().map(Vec::len).collect(),
            marks: vec![0; count],
            remaining: vec![0; count],
            generation: 0,
            cache: FxHashMap::default(),
        }
    }

    pub(super) fn covers(&mut self, common: usize, sites: &[BlockId]) -> bool {
        let mut key = sites.to_vec();
        key.sort_unstable();
        if let Some(&result) = self.cache.get(&key) {
            return result;
        }
        self.generation += 1;
        let generation = self.generation;
        let mut pending: Vec<_> = sites.iter().map(|site| site.0).collect();
        for &site in &pending {
            self.marks[site] = generation;
            self.remaining[site] = 0;
        }
        let mut budget = 4096;
        let mut covered = false;
        while let Some(block) = pending.pop() {
            if block == common {
                covered = true;
                break;
            }
            if budget == 0 {
                break;
            }
            budget -= 1;
            for &source in &self.predecessors[block] {
                if !self.dominators.dominates(common, source) {
                    continue;
                }
                if self.marks[source] != generation {
                    self.marks[source] = generation;
                    self.remaining[source] = self.successors[source];
                }
                if self.remaining[source] != 0 {
                    self.remaining[source] -= 1;
                    if self.remaining[source] == 0 {
                        pending.push(source);
                    }
                }
            }
        }
        self.cache.insert(key, covered);
        covered
    }
}
