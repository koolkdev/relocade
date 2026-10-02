//! Preserve branch specialization when placing shared calculations.
use super::*;
use std::collections::HashSet;

#[cfg(test)]
mod tests;

impl Placer<'_> {
    pub(super) fn materialize_shared(&mut self, block: BlockId) {
        let mut scheduled = std::mem::take(&mut self.shared[block.0]);
        if !scheduled.is_empty() {
            self.defer_eliminated(block, &mut scheduled);
        }
        for value in scheduled {
            self.materialize(value, block);
        }
    }

    fn defer_eliminated(&mut self, block: BlockId, scheduled: &mut Vec<usize>) {
        let Exit::If { condition, .. } = self.graph.blocks[block.0].exit else {
            return;
        };
        struct SharedValue {
            recipe: usize,
            residual: usize,
            can_defer: bool,
        }
        let mut values: Vec<_> = scheduled
            .iter()
            .map(|&recipe| SharedValue {
                recipe,
                residual: self.specialize(recipe, block),
                can_defer: false,
            })
            .collect();
        let exit_inputs = self.graph.blocks[block.0].exit.inputs();
        let exit_inputs: Vec<_> = exit_inputs
            .into_iter()
            .map(|value| self.specialize(value, block))
            .collect();
        let exit_needs = self.required_values(exit_inputs.iter().copied());
        for value in &mut values {
            let carrier = self.graph.values.representation(value.residual);
            value.can_defer = !exit_needs.contains(&carrier) && self.needs_evaluation(carrier);
        }
        if !values.iter().any(|value| value.can_defer) {
            return;
        }

        // Previews share the current availability but have separate facts and
        // residual caches. They neither schedule work nor create joined values.
        let mut paths = [false, true].map(|truth| {
            self.specializer
                .on_branch(&self.graph.values, &self.available, condition, truth)
        });
        for value in &mut values {
            value.can_defer = value.can_defer
                && paths.iter_mut().any(|path| {
                    let residual = path.specialize(self.graph, value.recipe, |graph, id| {
                        self.available.lookup(&graph.values, id)
                    });
                    !self.needs_evaluation(residual.value)
                });
        }

        // Retained work may still need a deferred root. Keep that dependency
        // early too, in the original schedule order.
        let needed = self.required_values(
            exit_inputs.into_iter().chain(
                values
                    .iter()
                    .filter(|value| !value.can_defer)
                    .map(|value| value.residual),
            ),
        );
        scheduled.clear();
        scheduled.extend(values.into_iter().filter_map(|value| {
            let carrier = self.graph.values.representation(value.residual);
            (!value.can_defer || needed.contains(&carrier)).then_some(value.recipe)
        }));
    }

    fn needs_evaluation(&self, value: usize) -> bool {
        let value = self.graph.values.representation(value);
        self.available.get(value).is_none()
            && !matches!(
                self.graph.values[value].definition,
                ValueDefinition::Constant(_) | ValueDefinition::Parameter { .. }
            )
    }

    fn required_values(&self, roots: impl IntoIterator<Item = usize>) -> HashSet<usize> {
        let mut pending: Vec<_> = roots.into_iter().collect();
        let mut needed = HashSet::new();
        while let Some(value) = pending.pop() {
            let value = self.graph.values.representation(value);
            if !needed.insert(value) || self.available.get(value).is_some() {
                continue;
            }
            if let Some(result) = self.graph.values.expression(value) {
                needed.insert(result.producer);
                pending.extend(result.expression.inputs().copied());
            }
        }
        needed
    }
}
