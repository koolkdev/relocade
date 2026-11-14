//! Reuse available executions through explicit block arguments.
use super::*;

impl Reuse<'_> {
    pub(super) fn may_join(&self, number: usize, block: BlockId) -> bool {
        let executions = self.executions[number].as_slice();
        let mut pending = Vec::new();
        for &(owner, _) in executions {
            if self.dominators.dominates(owner.0, block.0) {
                return true;
            }
            pending.extend(self.frontiers[owner.0].iter().copied());
        }
        let mut budget = 64;
        while let Some(join) = pending.pop() {
            if self.dominators.dominates(join, block.0) {
                return true;
            }
            if self.dominators.rank[join] > self.dominators.rank[block.0] {
                continue;
            }
            if budget == 0 {
                break;
            }
            budget -= 1;
            pending.extend(self.frontiers[join].iter().copied());
        }
        false
    }

    pub(super) fn join(
        &mut self,
        number: usize,
        block: BlockId,
        use_block: BlockId,
        budget: &mut usize,
    ) -> Option<(usize, bool)> {
        let executions = self.executions[number].as_slice();
        if *budget == 0 || executions.is_empty() {
            return None;
        }
        *budget -= 1;
        if let Some((_, value)) = executions
            .iter()
            .rev()
            .find(|(owner, _)| self.dominators.dominates(owner.0, block.0))
        {
            return Some((*value, false));
        }
        if self.incoming[block.0].is_empty()
            || self.incoming[block.0]
                .iter()
                .any(|&source| !self.complete[source])
        {
            return None;
        }
        let mut incoming = Vec::new();
        let mut guarded = false;
        for source in self.incoming[block.0].clone() {
            let value = if let Some((value, partial)) =
                self.join(number, BlockId(source), use_block, budget)
            {
                guarded |= partial;
                Some(value)
            } else if self.proofs.excludes(use_block, BlockId(source)) {
                guarded = true;
                None
            } else {
                return None;
            };
            incoming.push((source, value));
        }
        let first = incoming.iter().find_map(|(_, value)| *value)?;
        if incoming.iter().all(|(_, value)| *value == Some(first)) {
            return Some((first, guarded));
        }
        let ty = self.graph.values[first].ty;
        let incoming: Vec<_> = incoming
            .into_iter()
            .map(|(source, value)| {
                (
                    source,
                    value.unwrap_or_else(|| self.graph.values.carrier_literal(ty, 0_u64)),
                )
            })
            .collect();
        let component = self.graph.blocks[block.0].parameters.len();
        let bounds = incoming
            .iter()
            .map(|(_, value)| self.graph.values.bounds[*value])
            .reduce(BitBounds::union)
            .unwrap();
        let parameter = self.graph.values.push_with_bounds(
            Value {
                ty: self.graph.values[first].ty,
                definition: ValueDefinition::Parameter { block, component },
            },
            bounds,
        );
        self.graph.blocks[block.0].parameters.push(parameter);
        for (source, value) in incoming {
            for edge in self.graph.blocks[source].exit.edges_mut() {
                if edge.target == block {
                    edge.arguments.push(value);
                }
            }
        }
        self.extend();
        self.numbers.assign(parameter, number);
        // Placeholder inputs are irrelevant only under the consuming block's
        // proof. This equality must never become available at the earlier join.
        self.executions[number].push((if guarded { use_block } else { block }, parameter));
        Some((parameter, guarded))
    }
}
