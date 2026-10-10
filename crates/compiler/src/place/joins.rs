//! Preserve common facts and reuse incoming values at completed joins.
use super::{analysis::PathSnapshot, *};
use crate::{body::BitBounds, Type};
use std::collections::HashSet;

mod incoming;
use incoming::IncomingValues;
#[cfg(test)]
mod tests;

#[derive(Default)]
struct BlockValues {
    // Bindings established here, including residual recipes, relative to the dominator.
    values: HashMap<usize, usize>,
    facts: Option<PathSnapshot>,
}

struct JoinedValue {
    value: usize,
    // This value is only a placeholder for the recipe on these incoming paths.
    // Every use must prove they cannot coincide with its own path facts.
    excluded: Vec<usize>,
}

struct JoinInputs {
    incoming: Vec<IncomingValues>,
    results: HashMap<usize, JoinedValue>,
}

/// Validated arguments, including placeholders restricted to excluded paths.
struct JoinArguments {
    ty: Type,
    bounds: BitBounds,
    values: HashMap<usize, usize>,
    excluded: Vec<usize>,
}

pub(super) struct Joins {
    dominators: Dominators,
    predecessors: Vec<Vec<usize>>,
    blocks: Vec<BlockValues>,
    incoming: Vec<Vec<usize>>,
    needs_facts: Vec<bool>,
    inputs: Vec<Option<JoinInputs>>,
    joins_by_recipe: Vec<Vec<usize>>,
}

impl Joins {
    pub(super) fn new(
        graph: &FunctionGraph,
        predecessors: &[Vec<usize>],
        dominators: Dominators,
    ) -> Self {
        let count = predecessors.len();
        let mut needs_facts = vec![false; count];
        for sources in predecessors.iter().filter(|sources| sources.len() > 1) {
            for &source in sources {
                needs_facts[source] = true;
            }
        }
        let mut incoming = vec![Vec::new(); count];
        for (source, block) in graph.blocks.iter().enumerate() {
            for edge in block.exit.edges() {
                if !incoming[edge.target.0].contains(&source) {
                    incoming[edge.target.0].push(source);
                }
            }
        }
        Self {
            dominators,
            predecessors: predecessors.to_vec(),
            blocks: (0..count).map(|_| BlockValues::default()).collect(),
            incoming,
            needs_facts,
            inputs: (0..count).map(|_| None).collect(),
            joins_by_recipe: vec![Vec::new(); graph.values.len()],
        }
    }

    pub(super) fn record(&mut self, block: usize, values: impl Iterator<Item = (usize, usize)>) {
        self.blocks[block].values.extend(values);
    }

    pub(super) fn complete(&mut self, block: usize, facts: &ValueAnalysis) {
        if self.needs_facts[block] {
            // These facts outlive the block's active path scope.
            debug_assert!(self.blocks[block].facts.is_none());
            self.blocks[block].facts = Some(facts.snapshot(block));
        }
    }

    /// Prepare incoming facts and forward sole-edge results before placing consumers.
    pub(super) fn prepare(
        &mut self,
        graph: &FunctionGraph,
        reachable: &[bool],
        join: usize,
        available: &mut Availability,
    ) -> Option<ValueAnalysis> {
        if self.predecessors[join].len() < 2 {
            if let [source] = self.predecessors[join].as_slice() {
                if *source != join && self.dominators.dominates(*source, join) {
                    forward_parameters(graph, join, &[*source], available);
                }
            }
            return None;
        }
        // Placement can fold exits after the original dominance analysis.
        // Only paths that can still enter constrain the join's facts and values.
        let sources: Vec<_> = self.predecessors[join]
            .iter()
            .copied()
            .filter(|&source| {
                reachable[source]
                    && graph
                        .outgoing(BlockId(source))
                        .iter()
                        .any(|edge| edge.target.0 == join)
            })
            .collect();
        // An unfinished incoming block can be a loop backedge. Completing it
        // later must not make another iteration's facts or values valid on entry.
        if sources.is_empty()
            || sources
                .iter()
                .any(|&source| self.blocks[source].facts.is_none())
        {
            return None;
        }
        let incoming: Vec<_> = sources
            .iter()
            .flat_map(|&source| {
                let facts = self.blocks[source].facts.as_ref().unwrap().analysis();
                graph
                    .outgoing(BlockId(source))
                    .into_iter()
                    .filter(move |edge| edge.target.0 == join)
                    .map(move |edge| (facts, edge.arguments.as_slice()))
            })
            .collect();
        let facts = ValueAnalysis::join(&graph.values, &graph.blocks[join].parameters, &incoming);
        // An incoming block's arguments have already been placed. When folding
        // leaves one edge, consumers can use them directly instead of a join.
        forward_parameters(graph, join, &sources, available);
        let common = self.dominators.parent[join].unwrap();
        let mut incoming = Vec::new();
        let mut candidates = HashSet::new();
        for source in sources {
            let mut block = source;
            while block != common {
                for &recipe in self.blocks[block].values.keys() {
                    if available.get(recipe).is_none() && graph.values.expression(recipe).is_some()
                    {
                        candidates.insert(recipe);
                    }
                }
                block = self.dominators.parent[block].unwrap();
            }
            incoming.push(IncomingValues::new(source));
        }
        if candidates.is_empty() {
            return Some(facts);
        }
        for recipe in candidates {
            if recipe >= self.joins_by_recipe.len() {
                self.joins_by_recipe.resize_with(recipe + 1, Vec::new);
            }
            self.joins_by_recipe[recipe].push(join);
        }
        self.inputs[join] = Some(JoinInputs {
            incoming,
            results: HashMap::new(),
        });
        Some(facts)
    }

    /// Resolve incoming values, adding a parameter and edge arguments when needed.
    /// Missing paths may carry a placeholder only when the use's facts exclude them.
    pub(super) fn resolve(
        &mut self,
        graph: &mut FunctionGraph,
        block: usize,
        recipe: usize,
        facts: &ValueAnalysis,
    ) -> Option<usize> {
        let candidates = self.joins_by_recipe.get(recipe)?;
        // Prefer a previously constructed value before adding another parameter.
        for &join in candidates.iter().rev() {
            if !self.dominators.dominates(join, block) {
                continue;
            }
            if let Some(result) = self.inputs[join].as_ref().unwrap().results.get(&recipe) {
                if result.usable_under(&graph.values, &self.blocks, facts) {
                    return Some(result.value);
                }
            }
        }
        // Only joins with a recorded incoming value can supply this recipe.
        // Reverse traversal tries nearer ancestors first and excludes siblings.
        for index in (0..candidates.len()).rev() {
            let join = self.joins_by_recipe[recipe][index];
            if !self.dominators.dominates(join, block) {
                continue;
            }
            // The first pass already checked this cached value's path requirements.
            if self.inputs[join]
                .as_ref()
                .unwrap()
                .results
                .contains_key(&recipe)
            {
                continue;
            }
            let mut inputs = self.inputs[join].take().unwrap();
            let arguments = inputs.resolve_arguments(graph, recipe, facts, self);
            self.inputs[join] = Some(inputs);
            let Some(arguments) = arguments else {
                continue;
            };
            let result = arguments.materialize(graph, BlockId(join), &self.incoming[join]);
            let value = result.value;
            // A guarded value represents this recipe only at justified
            // uses. Unconditional values also belong to the owning join.
            if result.excluded.is_empty() {
                self.blocks[join].values.insert(recipe, value);
            }
            self.inputs[join]
                .as_mut()
                .unwrap()
                .results
                .insert(recipe, result);
            return Some(value);
        }
        None
    }

    fn recorded_value(&self, mut block: usize, recipe: usize) -> Option<usize> {
        loop {
            if let Some(&value) = self.blocks[block].values.get(&recipe) {
                return Some(value);
            }
            let parent = self.dominators.parent[block]?;
            if parent == block {
                return None;
            }
            block = parent;
        }
    }
}

fn forward_parameters(
    graph: &FunctionGraph,
    join: usize,
    sources: &[usize],
    available: &mut Availability,
) {
    if graph.blocks[join].parameters.is_empty() {
        return;
    }
    let mut edges = sources
        .iter()
        .flat_map(|&source| graph.outgoing(BlockId(source)))
        .filter(|edge| edge.target.0 == join);
    let Some(edge) = edges.next() else {
        return;
    };
    if edges.next().is_some() {
        return;
    }
    for (&parameter, &argument) in graph.blocks[join].parameters.iter().zip(&edge.arguments) {
        available.bind(parameter, argument);
    }
}

impl JoinedValue {
    fn usable_under(
        &self,
        values: &ValueTable,
        blocks: &[BlockValues],
        facts: &ValueAnalysis,
    ) -> bool {
        self.excluded
            .iter()
            .all(|&source| facts.excludes(values, blocks[source].facts.as_ref().unwrap()))
    }
}

impl JoinInputs {
    /// Resolve and validate every incoming argument before changing the join.
    fn resolve_arguments(
        &mut self,
        graph: &mut FunctionGraph,
        recipe: usize,
        facts: &ValueAnalysis,
        joins: &Joins,
    ) -> Option<JoinArguments> {
        let ty = graph.values[recipe].ty;
        let mut values = HashMap::new();
        let mut excluded = Vec::new();
        for source in &mut self.incoming {
            let value = if let Some(value) = source.resolve(graph, recipe, joins) {
                value
            } else if facts.excludes(
                &graph.values,
                joins.blocks[source.source].facts.as_ref().unwrap(),
            ) {
                excluded.push(source.source);
                graph.values.literal(ty, 0)
            } else {
                return None;
            };
            values.insert(source.source, value);
        }
        if !excluded.is_empty()
            && !self.incoming.iter().any(|source| {
                !excluded.contains(&source.source)
                    && !facts.excludes(
                        &graph.values,
                        joins.blocks[source.source].facts.as_ref().unwrap(),
                    )
            })
        {
            return None;
        }
        let bounds = values
            .values()
            .map(|&value| graph.values.bounds[value])
            .reduce(BitBounds::union)
            .unwrap();
        let promised = graph.values.bounds[recipe];
        if bounds.unsigned > promised.unsigned
            || bounds.signed > promised.signed
            || values
                .values()
                .any(|&value| graph.values[value].ty.carrier() != ty.carrier())
        {
            return None;
        }
        Some(JoinArguments {
            ty,
            bounds,
            values,
            excluded,
        })
    }
}

impl JoinArguments {
    fn materialize(
        self,
        graph: &mut FunctionGraph,
        join: BlockId,
        incoming: &[usize],
    ) -> JoinedValue {
        let first = *self.values.values().next().unwrap();
        if self.values.values().all(|&value| value == first) {
            return JoinedValue {
                value: first,
                excluded: self.excluded,
            };
        }
        let component = graph.blocks[join.0].parameters.len();
        let parameter = graph.values.push_with_bounds(
            Value {
                ty: self.ty,
                definition: ValueDefinition::Parameter {
                    block: join,
                    component,
                },
            },
            self.bounds,
        );
        graph.blocks[join.0].parameters.push(parameter);
        for &source in incoming {
            let argument = self
                .values
                .get(&source)
                .copied()
                // Inactive edges still have well-formed tuples. Their
                // values are never consumed by the reachable join.
                .unwrap_or_else(|| graph.values.literal(self.ty, 0));
            for edge in graph.blocks[source].exit.edges_mut() {
                if edge.target == join {
                    edge.arguments.push(argument);
                }
            }
        }
        JoinedValue {
            value: parameter,
            excluded: self.excluded,
        }
    }
}
