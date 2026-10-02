//! Schedule the residual calculations left by path specialization.
use super::*;

impl Placer<'_> {
    /// Keep bindings for intermediate recipes before the next block discards
    /// its predecessor's specialization cache. Only executed residuals qualify.
    pub(super) fn record_materialized_aliases(&mut self) {
        let aliases: Vec<_> = self
            .specializer
            .residuals()
            .filter_map(|(&recipe, residual)| {
                if self.available.contains_key(&recipe) {
                    return None;
                }
                self.available.get(residual).map(|&value| (recipe, value))
            })
            .collect();
        for (recipe, value) in aliases {
            self.define(recipe, value);
        }
    }

    pub(super) fn specialize(&mut self, root: usize, block: BlockId) -> usize {
        self.specializer.specialize(self.graph, root, |graph, id| {
            if let Some(&result) = self.available.get(&id) {
                return Some(result);
            }
            if let Some(result) = placed_expression(&self.expressions, &graph.values, id) {
                return Some(result);
            }
            graph.values.expression(id)?;
            let result = self.joins.available_at(graph, block.0, id)?;
            self.available_log
                .push((id, self.available.insert(id, result)));
            Some(result)
        })
    }

    pub(super) fn materialize(&mut self, root: usize, block: BlockId) -> usize {
        let residual = self.specialize(root, block);
        let mut work = vec![(residual, false)];
        while let Some((id, ready)) = work.pop() {
            if self.available.contains_key(&id) {
                continue;
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
                continue;
            };
            let expression = result.expression;
            if !ready {
                work.push((id, true));
                work.extend(expression.inputs().rev().map(|&v| (v, false)));
                continue;
            }
            let expression = expression.map(|v| self.available.get(v).copied().unwrap_or(*v));
            let key = Value {
                ty: self.graph.values[result.producer].ty,
                definition: ValueDefinition::Expression(expression),
            };
            let placed = if let Some(&placed) = self.expressions.get(&key) {
                placed
            } else {
                let placed = self.graph.values.push(key);
                self.graph.blocks[block.0]
                    .items
                    .push(BlockItem::Evaluate(placed));
                self.expression_log
                    .push((key, self.expressions.insert(key, placed)));
                placed
            };
            // Publishing the whole group records an execution, independently
            // of any facts or aliases already known for individual components.
            for (recipe, output) in self
                .graph
                .values
                .expression_results(result.producer)
                .zip(self.graph.values.expression_results(placed))
            {
                self.define(output, output);
                self.define(recipe, output);
            }
        }
        let result = self.available.get(&residual).copied().unwrap_or(residual);
        self.define(root, result);
        result
    }

    pub(super) fn define(&mut self, id: usize, result: usize) {
        self.available_log
            .push((id, self.available.insert(id, result)));
    }
}

/// CSE is keyed by the producer's operation; a result keeps its own component.
pub(super) fn placed_expression(
    expressions: &HashMap<Value, usize>,
    values: &ValueTable,
    id: usize,
) -> Option<usize> {
    let result = values.expression(id)?;
    let &producer = expressions.get(&values[result.producer])?;
    Some(values.expression_result(producer, result.component))
}
