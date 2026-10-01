//! Reuse incoming values at a join when a calculation needs them.
use super::*;
use crate::{integer::BitBounds, Type};
use std::collections::HashSet;

mod incoming;
use incoming::IncomingValues;
#[cfg(test)]
mod tests;

#[derive(Default)]
struct BlockValues {
    // Only original-value bindings established here, relative to the dominator.
    values: HashMap<usize, usize>,
    facts: Option<Facts>,
}

struct JoinInputs {
    common: usize,
    incoming: Vec<IncomingValues>,
    results: HashMap<usize, Option<usize>>,
}

pub(super) struct Joins {
    dominators: Dominators,
    predecessors: Vec<Vec<usize>>,
    blocks: Vec<BlockValues>,
    incoming: Vec<Vec<usize>>,
    needs_facts: Vec<bool>,
    inputs: Vec<Option<JoinInputs>>,
    joins_by_recipe: Vec<Vec<usize>>,
    original_values: usize,
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
            original_values: graph.values.len(),
        }
    }

    pub(super) fn record(&mut self, block: usize, values: impl Iterator<Item = (usize, usize)>) {
        self.blocks[block]
            .values
            .extend(values.filter(|&(recipe, _)| recipe < self.original_values));
    }

    pub(super) fn complete(&mut self, block: usize, facts: Facts) {
        if self.needs_facts[block] {
            self.blocks[block].facts = Some(facts);
        }
    }

    /// Freeze eligible incoming paths on entry, in dominator traversal order.
    pub(super) fn prepare(
        &mut self,
        graph: &FunctionGraph,
        join: usize,
        base: &HashMap<usize, usize>,
    ) {
        let sources = &self.predecessors[join];
        // An unfinished incoming block can be a loop backedge. Completing it
        // later must not make another iteration's value available on entry.
        if sources.len() < 2
            || sources
                .iter()
                .any(|&source| self.blocks[source].facts.is_none())
        {
            return;
        }
        let common = self.dominators.parent[join].unwrap();
        let mut incoming = Vec::new();
        let mut candidates = HashSet::new();
        for &source in sources {
            let mut delta = HashMap::new();
            let mut block = source;
            while block != common {
                for (&recipe, &value) in &self.blocks[block].values {
                    if !base.contains_key(&recipe) {
                        delta.entry(recipe).or_insert(value);
                        if graph.values.expression(recipe).is_some() {
                            candidates.insert(recipe);
                        }
                    }
                }
                block = self.dominators.parent[block].unwrap();
            }
            incoming.push(IncomingValues::new(source, delta));
        }
        if candidates.is_empty() {
            return;
        }
        for recipe in candidates {
            self.joins_by_recipe[recipe].push(join);
        }
        self.inputs[join] = Some(JoinInputs {
            common,
            incoming,
            results: HashMap::new(),
        });
    }

    pub(super) fn available_at(
        &mut self,
        graph: &mut FunctionGraph,
        block: usize,
        recipe: usize,
    ) -> Option<usize> {
        if recipe >= self.original_values {
            return None;
        }
        // Only joins with a recorded incoming value can supply this recipe.
        // Reverse traversal tries nearer ancestors first and excludes siblings.
        for index in (0..self.joins_by_recipe[recipe].len()).rev() {
            let join = self.joins_by_recipe[recipe][index];
            if !self.dominators.dominates(join, block) {
                continue;
            }
            if let Some(&result) = self.inputs[join].as_ref().unwrap().results.get(&recipe) {
                if result.is_some() {
                    return result;
                }
                continue;
            }
            let mut inputs = self.inputs[join].take().unwrap();
            let arguments = inputs
                .incoming
                .iter_mut()
                .map(|source| source.resolve(graph, recipe, inputs.common, self))
                .collect::<Option<Vec<_>>>();
            let result = arguments.and_then(|args| self.join_arguments(graph, join, recipe, &args));
            inputs.results.insert(recipe, result);
            self.inputs[join] = Some(inputs);
            if let Some(value) = result {
                // A child can be the first requester. Record the binding at
                // its owning join so other dominated uses can also find it.
                self.blocks[join].values.insert(recipe, value);
                return Some(value);
            }
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

    fn join_arguments(
        &self,
        graph: &mut FunctionGraph,
        join: usize,
        recipe: usize,
        arguments: &[usize],
    ) -> Option<usize> {
        let bounds = arguments
            .iter()
            .map(|&value| graph.values.bounds[value])
            .reduce(BitBounds::union)
            .unwrap();
        let promised = graph.values.bounds[recipe];
        let ty = graph.values[recipe].ty;
        if bounds.unsigned > promised.unsigned
            || bounds.signed > promised.signed
            || arguments
                .iter()
                .any(|&value| (graph.values[value].ty == Type::I64) != (ty == Type::I64))
        {
            return None;
        }
        if arguments.iter().all(|&value| value == arguments[0]) {
            return Some(arguments[0]);
        }
        let component = graph.blocks[join].parameters.len();
        let parameter = graph.values.push_with_bounds(
            Value {
                ty,
                definition: ValueDefinition::Parameter {
                    block: BlockId(join),
                    component,
                },
            },
            bounds,
        );
        graph.blocks[join].parameters.push(parameter);
        for &source in &self.incoming[join] {
            let argument = self.predecessors[join]
                .iter()
                .position(|&predecessor| predecessor == source)
                .map(|index| arguments[index])
                // Inactive edges still have well-formed tuples. Their
                // values are never consumed by the reachable join.
                .unwrap_or_else(|| graph.values.constant(ty, 0));
            for edge in graph.blocks[source].exit.edges_mut() {
                if edge.target.0 == join {
                    edge.arguments.push(argument);
                }
            }
        }
        Some(parameter)
    }
}
