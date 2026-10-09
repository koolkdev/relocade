//! Fold small Boolean combinations using shared select alternatives.

use super::ScalarFacts;
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    Expression, Type,
};

const MAX_VALUES: usize = 32;
const MAX_CASES: usize = 8;

#[cfg(test)]
mod tests;

impl ScalarFacts {
    pub(in crate::place) fn constant_across_selects(
        &mut self,
        table: &ValueTable,
        root: usize,
    ) -> Option<u64> {
        // Spend case-analysis work where Boolean predicates combine; ordinary
        // inference handles leaves. Require canonical predicates because a
        // narrow view alone does not establish its unused carrier bits.
        if table[root].ty != Type::I1
            || table.bounds[root].unsigned > 1
            || !matches!(
                table[root].definition,
                ValueDefinition::Expression(Expression::Bitwise {
                    operator: BitwiseOp::And | BitwiseOp::Or | BitwiseOp::Xor,
                    ..
                })
            )
        {
            return None;
        }
        if let Some(value) = self.inferred_constant(table, root) {
            return Some(value);
        }
        if let Some(&result) = self.select_constants.get(&root) {
            return result;
        }
        let result = self.select_inputs(table, root).and_then(|conditions| {
            if conditions.is_empty() {
                return None;
            }
            // Case assumptions use the existing undo scopes. Suspend inference
            // caches so speculative invalidation leaves the caller's cache intact.
            let computed = self.computed.take();
            let constants = std::mem::take(&mut self.select_constants);
            let mut remaining = MAX_CASES;
            let result = self.prove_select_cases(table, root, &conditions, &mut remaining);
            *self.computed.get_mut() = computed;
            self.select_constants = constants;
            result
        });
        self.select_constants.insert(root, result);
        result
    }

    fn select_inputs(&self, table: &ValueTable, root: usize) -> Option<Vec<usize>> {
        let mut pending = vec![root];
        let mut visited = Vec::with_capacity(MAX_VALUES);
        let mut conditions = Vec::<(usize, usize)>::new();
        while let Some(id) = pending.pop() {
            if visited.contains(&id) {
                continue;
            }
            if visited.len() == MAX_VALUES {
                return None;
            }
            visited.push(id);
            if matches!(table[id].definition, ValueDefinition::Literal(_))
                || self
                    .known
                    .get(&id)
                    .is_some_and(|bits| bits.mask == table[id].ty.mask())
            {
                continue;
            }
            let Some(result) = table.expression(id) else {
                continue;
            };
            if let Expression::Select {
                condition,
                when_true,
                when_false,
            } = result.expression
            {
                pending.push(condition);
                if let Some(truth) = self.inferred_constant(table, condition) {
                    pending.push(if truth == 0 { when_false } else { when_true });
                } else {
                    if let Some((_, uses)) = conditions.iter_mut().find(|(id, _)| *id == condition)
                    {
                        *uses += 1;
                    } else {
                        conditions.push((condition, 1));
                    }
                    pending.extend([when_true, when_false]);
                }
            } else {
                pending.extend(result.expression.inputs());
            }
        }
        // Reserve case analysis for values sharing a selector.
        conditions.retain(|(_, uses)| *uses > 1);
        // Shared selectors expose correlations first. IDs give tied selectors
        // a stable order within the proof budget.
        conditions.sort_unstable_by_key(|&(id, uses)| (std::cmp::Reverse(uses), id));
        Some(conditions.into_iter().map(|(id, _)| id).collect())
    }

    fn prove_select_cases(
        &mut self,
        table: &ValueTable,
        root: usize,
        conditions: &[usize],
        remaining: &mut usize,
    ) -> Option<u64> {
        *remaining = remaining.checked_sub(1)?;
        if let Some(value) = self.inferred_constant(table, root) {
            return Some(value);
        }
        let (&condition, rest) = conditions.split_first()?;
        if self.inferred_constant(table, condition).is_some() {
            return self.prove_select_cases(table, root, rest, remaining);
        }
        let mut results = [None; 2];
        for (truth, result) in results.iter_mut().enumerate() {
            let scope = self.checkpoint();
            self.assume(table, condition, truth != 0);
            *result = self.prove_select_cases(table, root, rest, remaining);
            self.restore(scope);
            (*result)?;
        }
        (results[0] == results[1]).then_some(results[0]).flatten()
    }
}
