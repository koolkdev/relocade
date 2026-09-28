//! Merge already available values once, where their control paths meet.
use super::*;
use crate::{integer::BitBounds, Type};
use std::collections::HashSet;

#[derive(Default)]
struct BlockValues {
    // Only original-value bindings established here, relative to the dominator.
    values: HashMap<usize, usize>,
    facts: Option<Facts>,
}

pub(super) struct Joins {
    dominators: Dominators,
    predecessors: Vec<Vec<usize>>,
    blocks: Vec<BlockValues>,
    incoming: Vec<Vec<usize>>,
    needs_facts: Vec<bool>,
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

    pub(super) fn merge(
        &self,
        graph: &mut FunctionGraph,
        join: usize,
        base: &HashMap<usize, usize>,
    ) -> Vec<(usize, usize)> {
        let sources = &self.predecessors[join];
        // An unfinished incoming block can be a loop backedge. Its execution
        // belongs to another iteration and cannot supply a value on entry.
        if sources.len() < 2
            || sources
                .iter()
                .any(|&source| self.blocks[source].facts.is_none())
        {
            return Vec::new();
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
                        if matches!(
                            graph.values[recipe].definition,
                            ValueDefinition::Expression(_)
                        ) {
                            candidates.insert(recipe);
                        }
                    }
                }
                block = self.dominators.parent[block].unwrap();
            }
            incoming.push(delta);
        }
        let mut candidates: Vec<_> = candidates.into_iter().collect();
        candidates.sort_unstable();
        let mut merged = Vec::new();
        for recipe in candidates {
            let Some(arguments) = sources
                .iter()
                .zip(&incoming)
                .map(|(&source, delta)| self.resolve(graph, source, recipe, delta, base))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
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
                continue;
            }
            if arguments.iter().all(|&value| value == arguments[0]) {
                merged.push((recipe, arguments[0]));
                continue;
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
                let argument = sources
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
            merged.push((recipe, parameter));
        }
        merged
    }

    fn resolve(
        &self,
        graph: &mut FunctionGraph,
        source: usize,
        mut recipe: usize,
        delta: &HashMap<usize, usize>,
        base: &HashMap<usize, usize>,
    ) -> Option<usize> {
        let facts = self.blocks[source].facts.as_ref()?;
        loop {
            if let Some(&value) = delta.get(&recipe).or_else(|| base.get(&recipe)) {
                return Some(value);
            }
            let value = graph.values[recipe];
            if let ValueDefinition::Parameter { block, .. } = value.definition {
                return self.dominators.dominates(block.0, source).then_some(recipe);
            }
            if let Some(bits) = facts.constant(&graph.values, recipe) {
                return Some(graph.values.intern(Value {
                    ty: value.ty,
                    definition: ValueDefinition::Constant(graph.values.carrier_bits(recipe, bits)),
                }));
            }
            recipe = match value.definition {
                ValueDefinition::Expression(Expression::Select {
                    condition,
                    when_true,
                    when_false,
                }) => {
                    if facts.constant(&graph.values, condition)? == 0 {
                        when_false
                    } else {
                        when_true
                    }
                }
                ValueDefinition::Expression(Expression::Convert { input })
                    if (graph.values[input].ty == Type::I64) == (value.ty == Type::I64) =>
                {
                    input
                }
                _ => return None,
            };
        }
    }
}
