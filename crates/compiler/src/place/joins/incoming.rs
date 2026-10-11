//! Reduce a predecessor recipe with the same semantics used by its consumers.
use super::*;

pub(super) struct IncomingValues {
    pub(super) source: usize,
    reducer: Reducer,
    resolved: HashMap<usize, Option<usize>>,
}

impl IncomingValues {
    pub(super) fn new(source: usize, facts: &ValueAnalysis) -> Self {
        Self {
            source,
            reducer: Reducer::from_analysis(facts.clone()),
            resolved: HashMap::new(),
        }
    }

    pub(super) fn resolve(
        &mut self,
        graph: &mut FunctionGraph,
        recipe: usize,
        joins: &Joins,
    ) -> Option<usize> {
        if let Some(&result) = self.resolved.get(&recipe) {
            return result;
        }
        // Reduction never adds an execution. A non-leaf residual can only be
        // supplied by a calculation already recorded on this predecessor.
        let residual = self.reducer.reduce(&mut graph.values, recipe);
        let residual = graph.values.representation(residual);
        let result = joins
            .recorded_value(self.source, recipe)
            .or_else(|| joins.recorded_value(self.source, residual))
            .or_else(|| match graph.values[residual].definition {
                ValueDefinition::Literal(_) => Some(residual),
                ValueDefinition::Parameter { block, .. }
                    if joins.dominators.dominates(block.0, self.source) =>
                {
                    Some(residual)
                }
                _ => None,
            });
        // Ancestor records may later gain equivalent bindings. A cached miss
        // can forgo reuse but cannot authorize an unavailable execution.
        self.resolved.insert(recipe, result);
        result
    }
}
