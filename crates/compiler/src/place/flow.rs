//! Reduce semantic control flow before deciding where its calculations execute.
use super::*;

#[cfg(test)]
mod tests;

/// Fixed semantic contexts and reductions, completed before placement demand.
/// Scheduling may request another normal form but cannot change a context.
pub(super) struct ReducedFlow {
    reducers: Vec<Option<Reducer>>,
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
            let incoming =
                incoming_analysis(graph, block, &sources, &reducers).unwrap_or_else(|| {
                    dominators.parent[block]
                        .filter(|&parent| parent != block)
                        .map(|parent| reducers[parent].as_ref().unwrap().analysis().clone())
                        .unwrap_or_default()
                });
            let edge = if let [source] = sources.as_slice() {
                match &exits[*source] {
                    Exit::If {
                        condition,
                        taken,
                        otherwise,
                    } if taken.target != otherwise.target => Some(Assumption::Truth {
                        condition: *condition,
                        truth: taken.target.0 == block,
                    }),
                    Exit::Switch {
                        selector,
                        cases,
                        default,
                    } if default.target.0 != block => {
                        let mut keys = cases
                            .iter()
                            .filter(|(_, edge)| edge.target.0 == block)
                            .map(|(key, _)| *key);
                        match (keys.next(), keys.next()) {
                            (Some(value), None) => Some(Assumption::Bits {
                                value: *selector,
                                mask: graph.values[*selector].ty.mask(),
                                bits: u64::from(value),
                            }),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            } else {
                None
            };
            let mut reducer =
                Reducer::from_analysis(incoming.with_assumptions(&graph.values, edge));
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
        ReducedFlow {
            reducers,
            control_changed,
        }
    }

    pub(super) fn analysis(&self, block: BlockId) -> &ValueAnalysis {
        self.reducers[block.0].as_ref().unwrap().analysis()
    }

    pub(super) fn reduce(
        &mut self,
        values: &mut ValueTable,
        block: BlockId,
        value: usize,
    ) -> usize {
        self.reducers[block.0]
            .as_mut()
            .unwrap()
            .reduce(values, value)
    }

    pub(super) fn recipes(&self, block: BlockId, value: usize) -> impl Iterator<Item = usize> {
        self.reducers[block.0].as_ref().unwrap().recipes(value)
    }
}

/// An unfinished predecessor is a possible backedge and contributes no entry proof.
fn incoming_analysis(
    graph: &FunctionGraph,
    block: usize,
    sources: &[usize],
    reducers: &[Option<Reducer>],
) -> Option<ValueAnalysis> {
    if sources.is_empty() || sources.iter().any(|&source| reducers[source].is_none()) {
        return None;
    }
    let incoming: Vec<_> = sources
        .iter()
        .flat_map(|&source| {
            let facts = reducers[source].as_ref().unwrap().analysis();
            graph
                .outgoing(BlockId(source))
                .into_iter()
                .filter(move |edge| edge.target.0 == block)
                .map(move |edge| (facts, edge.arguments.as_slice()))
        })
        .collect();
    Some(ValueAnalysis::join(
        &graph.values,
        &graph.blocks[block].parameters,
        &incoming,
    ))
}
