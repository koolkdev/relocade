//! Reuse may omit an incoming value only when explicit branch decisions exclude it.
use super::*;
use crate::ssa::versions::Versions;

pub(super) struct Paths {
    versions: Versions,
    outgoing: Vec<u32>,
    excluded: FxHashMap<(usize, usize), bool>,
}

impl Paths {
    pub(super) fn new(graph: &SsaGraph, dominators: &Dominators, incoming: &[Vec<usize>]) -> Self {
        let mut paths = Self {
            versions: Versions::new(),
            outgoing: vec![0; graph.blocks.len()],
            excluded: FxHashMap::default(),
        };
        let mut order: Vec<_> = (0..graph.blocks.len()).collect();
        order.sort_by_key(|&block| dominators.rank[block]);
        for block in order {
            let Some(parent) = dominators.parent[block] else {
                continue;
            };
            let mut root = paths.outgoing[parent];
            if let [source] = incoming[block][..] {
                if let Exit::If {
                    condition,
                    ref taken,
                    ref otherwise,
                } = graph.blocks[source].exit
                {
                    if taken.target != otherwise.target {
                        root = paths.versions.set(
                            root,
                            block,
                            condition << 1 | 1,
                            usize::from(taken.target.0 == block),
                        );
                    }
                }
            }
            paths.outgoing[block] = root;
        }
        paths
    }

    pub(super) fn excludes(&mut self, a: BlockId, b: BlockId) -> bool {
        let key = (a.0.min(b.0), a.0.max(b.0));
        *self.excluded.entry(key).or_insert_with(|| {
            self.versions
                .contradict(self.outgoing[a.0], self.outgoing[b.0])
        })
    }
}
