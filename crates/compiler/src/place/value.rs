//! Schedule the residual calculations left by path specialization.
use super::*;

impl Placer<'_> {
    pub(super) fn specialize(&mut self, root: usize, block: BlockId) -> usize {
        let specialized = self
            .specializer
            .specialize(self.graph, root, |graph, id, facts| {
                if let Some(result) = self.available.lookup(&graph.values, id) {
                    return Some(result);
                }
                graph.values.expression(id)?;
                let result = self.joins.resolve(graph, block.0, id, facts)?;
                self.available.bind(id, result);
                Some(result)
            });
        self.available.record_aliases(specialized.aliases);
        specialized.value
    }

    pub(super) fn materialize(&mut self, root: usize, block: BlockId) -> usize {
        let residual = self.specialize(root, block);
        let mut work = vec![(residual, false)];
        while let Some((id, ready)) = work.pop() {
            if self.available.get(id).is_some() {
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
                self.available.bind(id, id);
                continue;
            };
            let expression = result.expression;
            if !ready {
                work.push((id, true));
                work.extend(expression.inputs().rev().map(|&v| (v, false)));
                continue;
            }
            self.available.evaluate(self.graph, block, result.producer);
        }
        let result = self.available.get(residual).unwrap_or(residual);
        self.available.bind(root, result);
        result
    }
}
