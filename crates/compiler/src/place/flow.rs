//! Reduce semantic control flow before deciding where its calculations execute.
use super::*;
use rustc_hash::FxHashMap;

#[cfg(test)]
mod tests;

/// Fixed semantic contexts and reductions, completed before placement demand.
/// Scheduling may request another normal form but cannot change a context.
pub(super) struct ReducedFlow {
    reducer: Reducer,
    scopes: Vec<Option<ScopeId>>,
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
        let mut reducer = Reducer::new(graph.blocks.len());
        let mut scopes = vec![None; graph.blocks.len()];
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
            let incoming = incoming_facts(graph, block, &sources, &reducer, &scopes)
                .unwrap_or_else(|| {
                    dominators.parent[block]
                        .filter(|&parent| parent != block)
                        .map(|parent| reducer.facts(scopes[parent].unwrap()).clone())
                        .unwrap_or_default()
                });
            let mut observations: Vec<_> = if let [source] = sources.as_slice() {
                let authored = edge_observation(&graph.values, &exits[*source], BlockId(block));
                let reduced =
                    edge_observation(&graph.values, &graph.blocks[*source].exit, BlockId(block));
                // Both predicates describe the same edge. Keep authored facts for
                // original recipes and reduced facts for their new normal forms.
                [
                    authored,
                    reduced.filter(|observation| Some(*observation) != authored),
                ]
                .into_iter()
                .flatten()
                .collect()
            } else {
                Vec::new()
            };
            if let [source] = sources.as_slice() {
                let source = scopes[*source].unwrap();
                for observation in observations.clone() {
                    let value = match observation {
                        Assumption::Truth { condition, .. } => condition,
                        Assumption::Bits { value, .. } => value,
                    };
                    let residual = reducer.facts(source).reduced(value).unwrap_or(value);
                    for value in reducer.recipes(source, residual) {
                        let fact = match observation {
                            Assumption::Truth { truth, .. } => Assumption::Truth {
                                condition: value,
                                truth,
                            },
                            Assumption::Bits { mask, bits, .. } => Assumption::Bits {
                                value,
                                mask: mask & graph.values[value].ty.mask(),
                                bits,
                            },
                        };
                        if !observations.contains(&fact) {
                            observations.push(fact);
                        }
                    }
                }
            }
            let scope = reducer.scope(incoming.with_assumptions(&graph.values, observations));
            for item in &graph.blocks[block].items {
                let BlockItem::Effect(id) = item else {
                    unreachable!()
                };
                let operation = graph.effects[id.0]
                    .operation
                    .clone()
                    .map_inputs(|value| reducer.reduce(&mut graph.values, scope, value));
                graph.effects[id.0].operation = operation;
            }
            let before = graph.blocks[block].exit.constant_edge_index(&graph.values);
            let mut exit = graph.blocks[block].exit.clone();
            exit.map_inputs(|value| reducer.reduce(&mut graph.values, scope, value));
            graph.blocks[block].exit = exit;
            scopes[block] = Some(scope);
            if graph.blocks[block].exit.constant_edge_index(&graph.values) != before {
                control_changed = true;
                reachable = graph.reachable();
            }
        }
        ReducedFlow {
            reducer,
            scopes,
            exclusions: FxHashMap::default(),
            control_changed,
        }
    }

    pub(super) fn facts(&self, block: BlockId) -> &ScalarFacts {
        self.reducer.facts(self.scopes[block.0].unwrap())
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
        self.reducer
            .reduce(values, self.scopes[block.0].unwrap(), value)
    }

    pub(super) fn recipes(&self, block: BlockId, value: usize) -> impl Iterator<Item = usize> + '_ {
        self.reducer.recipes(self.scopes[block.0].unwrap(), value)
    }
}

/// An unfinished predecessor is a possible backedge and contributes no entry proof.
fn incoming_facts(
    graph: &FunctionGraph,
    block: usize,
    sources: &[usize],
    reducer: &Reducer,
    scopes: &[Option<ScopeId>],
) -> Option<ScalarFacts> {
    if sources.is_empty() || sources.iter().any(|&source| scopes[source].is_none()) {
        return None;
    }
    let incoming: Vec<_> = sources
        .iter()
        .flat_map(|&source| {
            let facts = reducer.facts(scopes[source].unwrap());
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
