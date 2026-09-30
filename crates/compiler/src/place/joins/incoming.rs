//! Resolve a recipe using one predecessor's values and branch facts.
use super::*;

pub(super) struct IncomingValues {
    source: usize,
    values: HashMap<usize, usize>,
    resolved: HashMap<usize, Option<usize>>,
}

impl IncomingValues {
    pub(super) fn new(source: usize, values: HashMap<usize, usize>) -> Self {
        Self {
            source,
            values,
            resolved: HashMap::new(),
        }
    }

    pub(super) fn resolve(
        &mut self,
        graph: &mut FunctionGraph,
        mut recipe: usize,
        common: usize,
        joins: &Joins,
    ) -> Option<usize> {
        let facts = joins.blocks[self.source].facts.as_ref().unwrap();
        let mut path = Vec::new();
        let result = loop {
            if let Some(&result) = self.resolved.get(&recipe) {
                break result;
            }
            path.push(recipe);
            // Incoming bindings win over the shared ancestor's bindings.
            // Start fallback at the saved common dominator, never at the use.
            if let Some(value) = self
                .values
                .get(&recipe)
                .copied()
                .or_else(|| joins.recorded_value(common, recipe))
            {
                break Some(value);
            }
            let value = graph.values[recipe];
            if let ValueDefinition::Parameter { block, .. } = value.definition {
                break joins
                    .dominators
                    .dominates(block.0, self.source)
                    .then_some(recipe);
            }
            if let Some(bits) = facts.constant(&graph.values, recipe) {
                let bits = graph.values.carrier_bits(recipe, bits);
                break Some(graph.values.carrier_constant(value.ty, bits));
            }
            recipe = match value.definition {
                ValueDefinition::Expression(Expression::Select {
                    condition,
                    when_true,
                    when_false,
                }) => {
                    let Some(bits) = facts.constant(&graph.values, condition) else {
                        break None;
                    };
                    if bits == 0 {
                        when_false
                    } else {
                        when_true
                    }
                }
                ValueDefinition::Expression(Expression::Convert { input })
                    if graph.values[input].ty.carrier() == value.ty.carrier() =>
                {
                    input
                }
                _ => break None,
            };
        };
        // Ancestor records can gain equivalent bindings after this query.
        // A cached miss may forgo reuse, but cannot introduce an invalid value.
        for recipe in path {
            self.resolved.insert(recipe, result);
        }
        result
    }
}
