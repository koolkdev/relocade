//! Fold small Boolean combinations using shared select alternatives.

use super::{Assumption, ScalarFacts};
use crate::expression::BinaryOp as Binary;
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    Expression, Type,
};

const MAX_VALUES: usize = 32;
const MAX_PROOF_STEPS: usize = 8;

#[cfg(test)]
mod tests;

impl ScalarFacts {
    pub(in crate::place) fn constant_across_selects(
        &self,
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
                ValueDefinition::Expression(Expression::Binary {
                    operator: Binary::Bitwise(BitwiseOp::And | BitwiseOp::Or | BitwiseOp::Xor),
                    ..
                })
            )
        {
            return None;
        }
        // Bound discovery before inference can walk a large unrelated predicate.
        let conditions = self.select_inputs(table, root)?;
        if let Some(value) = self.inferred_constant(table, root) {
            return Some(value);
        }
        if conditions.is_empty() {
            return None;
        }
        let mut remaining = MAX_PROOF_STEPS;
        self.prove_select_cases(table, root, &conditions, &mut remaining)
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
                if let Some((_, uses)) = conditions.iter_mut().find(|(id, _)| *id == condition) {
                    *uses += 1;
                } else {
                    conditions.push((condition, 1));
                }
                pending.extend([condition, when_true, when_false]);
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
        &self,
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
            let case = self.clone().with_assumptions(
                table,
                [Assumption::Truth {
                    condition,
                    truth: truth != 0,
                }],
            );
            *result = case.prove_select_cases(table, root, rest, remaining);
            (*result)?;
        }
        (results[0] == results[1]).then_some(results[0]).flatten()
    }
}
