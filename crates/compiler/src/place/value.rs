//! Schedule the residual calculations left by path specialization.
use super::*;

impl Placer<'_> {
    fn bind_reduction(&mut self, residual: usize, value: usize) {
        for recipe in self.reducer.recipes(residual) {
            if recipe == residual || self.available.get(recipe).is_none() {
                self.available.bind(recipe, value);
            }
        }
    }

    pub(super) fn materialize(&mut self, root: usize, block: BlockId) -> usize {
        let residual = self.reducer.reduce(&mut self.graph.values, root);
        let mut work = vec![(residual, false)];
        while let Some((id, ready)) = work.pop() {
            if let Some(value) = self.available.get(id) {
                self.bind_reduction(id, value);
                continue;
            }
            if self.graph.values.expression(id).is_some() {
                let candidates = self.reducer.recipes(id);
                let reused = candidates.into_iter().find_map(|recipe| {
                    self.available
                        .lookup(&self.graph.values, recipe)
                        .or_else(|| {
                            self.joins
                                .resolve(self.graph, block.0, recipe, self.reducer.analysis())
                        })
                });
                if let Some(value) = reused {
                    self.bind_reduction(id, value);
                    continue;
                }
            }
            let Some(result) = self.graph.values.expression(id) else {
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
                self.bind_reduction(id, id);
                continue;
            };
            let expression = result.expression;
            if !ready {
                work.push((id, true));
                work.extend(expression.inputs().rev().map(|&v| (v, false)));
                continue;
            }
            self.available.evaluate(self.graph, block, result.producer);
            for output in self.graph.values.expression_results(result.producer) {
                let value = self.available.get(output).unwrap();
                self.bind_reduction(output, value);
            }
        }
        let result = self.available.get(residual).unwrap_or(residual);
        self.available.bind(root, result);
        result
    }
}
