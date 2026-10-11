//! Schedule the residual calculations left by path specialization.
use super::*;

impl Placer<'_> {
    fn bind_reduction(&mut self, block: BlockId, residual: usize, value: usize) {
        for recipe in self.flow.recipes(block, residual) {
            if recipe == residual || self.available.get(recipe).is_none() {
                self.available.bind(recipe, value);
            }
        }
    }

    pub(super) fn materialize(&mut self, root: usize, block: BlockId) -> usize {
        let residual = self.flow.reduce(&mut self.graph.values, block, root);
        let root_recipe = self.plan.recipe(block, residual);
        let mut work = vec![(residual, false)];
        while let Some((id, ready)) = work.pop() {
            if let Some(value) = self.available.get(id) {
                self.bind_reduction(block, id, value);
                continue;
            }
            if self.graph.values.expression(id).is_some() {
                let candidates = self.flow.recipes(block, id);
                let reused = candidates.into_iter().find_map(|recipe| {
                    self.available
                        .lookup(&self.graph.values, recipe)
                        .or_else(|| {
                            self.joins.resolve(
                                self.graph,
                                block.0,
                                recipe,
                                self.flow.analysis(block),
                            )
                        })
                });
                if let Some(value) = reused {
                    self.bind_reduction(block, id, value);
                    continue;
                }
            }
            // Demand was scheduled for this root's chosen recipe. Its inputs
            // follow that DAG without further recipe selection.
            let recipe = if id == residual { root_recipe } else { id };
            let Some(result) = self.graph.values.expression(recipe) else {
                if let ValueDefinition::Result {
                    producer: BlockItem::Effect(effect),
                    ..
                } = self.graph.values[id].definition
                {
                    panic!(
                        "effect result {id} from {} unavailable in block {}",
                        effect.0, block.0
                    );
                }
                self.bind_reduction(block, id, id);
                continue;
            };
            let expression = result.expression;
            if !ready {
                work.push((id, true));
                work.extend(expression.inputs().rev().map(|&v| (v, false)));
                continue;
            }
            self.available.evaluate(self.graph, block, result.producer);
            if recipe != id {
                let value = self.available.get(recipe).unwrap();
                self.bind_reduction(block, id, value);
            }
            for output in self.graph.values.expression_results(result.producer) {
                let value = self.available.get(output).unwrap();
                self.bind_reduction(block, output, value);
            }
        }
        let result = self.available.get(residual).unwrap_or(residual);
        self.available.bind(root, result);
        result
    }
}
