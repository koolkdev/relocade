//! Reduce semantic control flow before deciding where its calculations execute.
use super::*;
use rustc_hash::FxHashMap;

#[cfg(test)]
mod tests;

/// Fixed semantic contexts and reductions, shared by blocks with unchanged facts.
/// Control-flow reduction completes before placement demand.
/// Scheduling may request another normal form but cannot change a context.
pub(super) struct ReducedFlow {
    reducers: Vec<Reducer>,
    contexts: Vec<Option<usize>>,
    exclusions: FxHashMap<(BlockId, BlockId), bool>,
    pub(super) control_changed: bool,
}

impl ReducedFlow {
    pub(super) fn new(
        graph: &mut FunctionGraph,
        dominators: &Dominators,
        predecessors: &[Vec<usize>],
    ) -> Self {
        let exits: Vec<_> = graph
            .blocks
            .iter()
            .map(|block| block.exit.clone())
            .collect();
        let mut flow = Self {
            reducers: Vec::new(),
            contexts: vec![None; graph.blocks.len()],
            exclusions: FxHashMap::default(),
            control_changed: false,
        };
        let mut order: Vec<_> = (0..graph.blocks.len()).collect();
        order.sort_by_key(|&block| dominators.rank[block]);
        // Reverse postorder is topological when every edge points forward.
        // Such blocks can decide reachability from already processed predecessors.
        let acyclic = predecessors.iter().enumerate().all(|(block, sources)| {
            sources
                .iter()
                .all(|&source| dominators.rank[source] < dominators.rank[block])
        });
        let mut reachable = graph.reachable();
        for block in order {
            if !reachable[block] {
                continue;
            }
            let sources: Vec<_> = predecessors[block]
                .iter()
                .copied()
                .filter(|&source| {
                    reachable[source]
                        && graph
                            .outgoing(BlockId(source))
                            .iter()
                            .any(|edge| edge.target.0 == block)
                })
                .collect();
            if acyclic && block != graph.entry.0 && sources.is_empty() {
                reachable[block] = false;
                continue;
            }
            let parent = dominators.parent[block].filter(|&parent| parent != block);
            let incoming = flow
                .incoming_facts(graph, block, &sources)
                .unwrap_or_else(|| {
                    parent
                        .map(|parent| flow.facts(BlockId(parent)).clone())
                        .unwrap_or_default()
                });
            let observations = if let [source] = sources.as_slice() {
                let authored = edge_observation(&graph.values, &exits[*source], BlockId(block));
                let reduced =
                    edge_observation(&graph.values, &graph.blocks[*source].exit, BlockId(block));
                // Both predicates describe the same edge. Keep authored facts for
                // original recipes and reduced facts for their new normal forms.
                [
                    authored,
                    reduced.filter(|observation| Some(*observation) != authored),
                ]
            } else {
                [None; 2]
            };
            let facts =
                incoming.with_assumptions(&graph.values, observations.into_iter().flatten());
            let inherited = match sources.as_slice() {
                [source] => flow.contexts[*source],
                _ => parent.and_then(|parent| flow.contexts[parent]),
            };
            let context = inherited
                .filter(|&context| flow.reducers[context].facts().same_context(&facts))
                .unwrap_or_else(|| {
                    let context = flow.reducers.len();
                    flow.reducers.push(Reducer::from_facts(facts));
                    context
                });
            flow.contexts[block] = Some(context);
            let reducer = &mut flow.reducers[context];
            for item in &graph.blocks[block].items {
                let BlockItem::Effect(id) = item else {
                    unreachable!()
                };
                let operation = graph.effects[id.0]
                    .operation
                    .clone()
                    .map_inputs(|value| reducer.reduce(&mut graph.values, value));
                graph.effects[id.0].operation = operation;
            }
            let before = graph.blocks[block].exit.constant_edge_index(&graph.values);
            let mut exit = graph.blocks[block].exit.clone();
            exit.map_inputs(|value| reducer.reduce(&mut graph.values, value));
            graph.blocks[block].exit = exit;
            if graph.blocks[block].exit.constant_edge_index(&graph.values) != before {
                flow.control_changed = true;
                if !acyclic {
                    // A disconnected cycle can still have incoming edges from itself.
                    reachable = graph.reachable();
                }
            }
        }
        flow
    }

    pub(super) fn facts(&self, block: BlockId) -> &ScalarFacts {
        self.reducers[self.contexts[block.0].unwrap()].facts()
    }

    /// Fixed facts and existing value definitions/bounds make proofs and misses reusable.
    /// Placement only appends values; compaction runs after this owner is dropped.
    /// The key is directional: inference may prove one relation but miss its reverse.
    pub(super) fn excludes(
        &mut self,
        values: &ValueTable,
        block: BlockId,
        source: BlockId,
    ) -> bool {
        if let Some(&excluded) = self.exclusions.get(&(block, source)) {
            return excluded;
        }
        let excluded = self.facts(source).conflicts_with(values, self.facts(block));
        self.exclusions.insert((block, source), excluded);
        excluded
    }

    pub(super) fn reduce(
        &mut self,
        values: &mut ValueTable,
        block: BlockId,
        value: usize,
    ) -> usize {
        self.reducers[self.contexts[block.0].unwrap()].reduce(values, value)
    }

    pub(super) fn recipes(&self, block: BlockId, value: usize) -> impl Iterator<Item = usize> + '_ {
        self.reducers[self.contexts[block.0].unwrap()].recipes(value)
    }

    /// An unfinished predecessor is a possible backedge and contributes no entry proof.
    fn incoming_facts(
        &self,
        graph: &FunctionGraph,
        block: usize,
        sources: &[usize],
    ) -> Option<ScalarFacts> {
        if sources.is_empty()
            || sources
                .iter()
                .any(|&source| self.contexts[source].is_none())
        {
            return None;
        }
        let incoming: Vec<_> = sources
            .iter()
            .flat_map(|&source| {
                let facts = self.facts(BlockId(source));
                graph
                    .outgoing(BlockId(source))
                    .into_iter()
                    .filter(move |edge| edge.target.0 == block)
                    .map(move |edge| (facts, edge.arguments.as_slice()))
            })
            .collect();
        Some(ScalarFacts::join(
            &graph.values,
            &graph.blocks[block].parameters,
            &incoming,
        ))
    }
}

/// A unique outgoing alternative contributes a truth or selector observation.
fn edge_observation(values: &ValueTable, exit: &Exit, target: BlockId) -> Option<Assumption> {
    match exit {
        Exit::If {
            condition,
            taken,
            otherwise,
        } if taken.target != otherwise.target => Some(Assumption::Truth {
            condition: *condition,
            truth: taken.target == target,
        }),
        Exit::Switch {
            selector,
            cases,
            default,
        } if default.target != target => {
            let mut keys = cases
                .iter()
                .filter(|(_, edge)| edge.target == target)
                .map(|(key, _)| *key);
            match (keys.next(), keys.next()) {
                (Some(value), None) => Some(Assumption::Bits {
                    value: *selector,
                    mask: values[*selector].ty.mask(),
                    bits: u64::from(value),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}
