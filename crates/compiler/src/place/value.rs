//! Schedule the residual calculations left by path specialization.
use super::*;

impl Placer<'_> {
    pub(super) fn specialize(&mut self, root: usize, block: BlockId) -> usize {
        self.specializer.specialize(self.graph, root, |graph, id| {
            if let Some(&result) = self.available.get(&id) {
                return Some(result);
            }
            if let Some(&result) = self.expressions.get(&graph.values[id]) {
                return Some(result);
            }
            if !matches!(graph.values[id].definition, ValueDefinition::Expression(_)) {
                return None;
            }
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
            let value = self.graph.values[id];
            let ValueDefinition::Expression(expression) = value.definition else {
                if let ValueDefinition::Result { effect, .. } = value.definition {
                    panic!(
                        "effect result {id} from {} unavailable in block {}",
                        effect.0, block.0
                    );
                }
                continue;
            };
            if !ready {
                work.push((id, true));
                work.extend(expression.inputs().rev().map(|&v| (v, false)));
                continue;
            }
            let expression = expression.map(|v| self.available.get(v).copied().unwrap_or(*v));
            let key = Value {
                ty: value.ty,
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
                self.define(placed, placed);
                placed
            };
            self.define(id, placed);
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
