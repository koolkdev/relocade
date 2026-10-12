//! Reduce semantic control flow before deciding where its calculations execute.
use super::lower::Lowering;
use super::*;
use rustc_hash::FxHashMap;

#[cfg(test)]
mod tests;

/// Fixed semantic contexts and reductions, completed before placement demand.
/// Scheduling may request another normal form but cannot change a context.
pub(super) struct ReducedFlow {
    reducers: Vec<Option<Reducer>>,
    exclusions: FxHashMap<(BlockId, BlockId), bool>,
    lowering: Option<Lowering>,
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
        let mut reducers: Vec<Option<Reducer>> = (0..graph.blocks.len()).map(|_| None).collect();
        let mut order: Vec<_> = (0..graph.blocks.len()).collect();
        order.sort_by_key(|&block| dominators.rank[block]);
        let mut reachable = graph.reachable();
        let mut control_changed = false;
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
            let incoming = incoming_facts(graph, block, &sources, &reducers).unwrap_or_else(|| {
                dominators.parent[block]
                    .filter(|&parent| parent != block)
                    .map(|parent| reducers[parent].as_ref().unwrap().facts().clone())
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
            let mut reducer = Reducer::from_facts(
                incoming.with_assumptions(&graph.values, observations.into_iter().flatten()),
            );
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
            reducers[block] = Some(reducer);
            if graph.blocks[block].exit.constant_edge_index(&graph.values) != before {
                control_changed = true;
                reachable = graph.reachable();
            }
        }
        let mut flow = ReducedFlow {
            reducers,
            exclusions: FxHashMap::default(),
            lowering: None,
            control_changed,
        };
        if flow
            .reducers
            .iter()
            .flatten()
            .any(|reducer| reducer.needs_lowering)
        {
            let lowering = flow.lowering.get_or_insert_default();
            lowering.prepare(&mut graph.values);
            for (block, data) in graph.blocks.iter_mut().enumerate() {
                if !reachable[block] {
                    continue;
                }
                let before = data.exit.constant_edge_index(&graph.values);
                data.exit.map_inputs(|value| lowering.value(value));
                flow.control_changed |= data.exit.constant_edge_index(&graph.values) != before;
                for item in &data.items {
                    let BlockItem::Effect(id) = item else {
                        unreachable!()
                    };
                    graph.effects[id.0].operation = graph.effects[id.0]
                        .operation
                        .clone()
                        .map_inputs(|value| lowering.value(value));
                }
            }
        }
        flow
    }

    pub(super) fn facts(&self, block: BlockId) -> &ScalarFacts {
        self.reducers[block.0].as_ref().unwrap().facts()
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
        let reducer = self.reducers[block.0].as_mut().unwrap();
        let result = reducer.reduce(values, value);
        if reducer.needs_lowering {
            let lowering = self.lowering.get_or_insert_default();
            lowering.prepare(values);
            lowering.value(result)
        } else {
            self.lowering
                .as_ref()
                .map_or(result, |lowering| lowering.value(result))
        }
    }

    pub(super) fn recipes(&self, block: BlockId, value: usize) -> impl Iterator<Item = usize> {
        let reducer = self.reducers[block.0].as_ref().unwrap();
        let alternatives = if let Some(lowering) = &self.lowering {
            lowering.recipes(reducer, value)
        } else {
            reducer.alternatives(value)
        };
        std::iter::once(value).chain(alternatives)
    }
}

/// An unfinished predecessor is a possible backedge and contributes no entry proof.
fn incoming_facts(
    graph: &FunctionGraph,
    block: usize,
    sources: &[usize],
    reducers: &[Option<Reducer>],
) -> Option<ScalarFacts> {
    if sources.is_empty() || sources.iter().any(|&source| reducers[source].is_none()) {
        return None;
    }
    let incoming: Vec<_> = sources
        .iter()
        .flat_map(|&source| {
            let facts = reducers[source].as_ref().unwrap().facts();
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
