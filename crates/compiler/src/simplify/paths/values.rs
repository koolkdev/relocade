//! Rebuild calculations at their uses; snapshots and effect results keep identity.

use super::Path;
use crate::{
    body::{Value, ValueDefinition, ValueTable},
    Expression,
};

impl Path {
    pub(super) fn value(&mut self, table: &mut ValueTable, root: &mut usize) {
        let mut pending = vec![(*root, false)];
        while let Some((id, ready)) = pending.pop() {
            if self.rewritten.contains_key(&id) {
                continue;
            }
            let value = table.values[id];
            if let Some(bits) = self.facts.constant(table, id) {
                let result = constant(table, id, bits);
                self.rewritten.insert(id, result);
                continue;
            }
            if let Some(&shared) = self.shared.get(&id) {
                // A known selection can use an input or snapshot directly.
                // Calculated arms retain the group's rewrite: exposing one
                // separately could make placement repeat it on the same path.
                let result = match table.values[shared].definition {
                    ValueDefinition::Expression(Expression::Select {
                        condition,
                        when_true,
                        when_false,
                    }) => match self.facts.constant(table, condition) {
                        Some(0) => when_false,
                        Some(_) => when_true,
                        None => shared,
                    },
                    _ => shared,
                };
                self.rewritten.insert(
                    id,
                    if matches!(
                        table.values[result].definition,
                        ValueDefinition::Expression(_)
                    ) {
                        shared
                    } else {
                        result
                    },
                );
                continue;
            }
            let ValueDefinition::Expression(expression) = value.definition else {
                self.rewritten.insert(id, id);
                continue;
            };
            if !ready {
                pending.push((id, true));
                for &input in expression.inputs() {
                    if !self.rewritten.contains_key(&input) {
                        pending.push((input, false));
                    }
                }
                continue;
            }
            let expression = expression.map(|input| self.rewritten[input]);
            let rebuilt = table.rebuild(value.ty, expression);
            let result = match self.facts.refine(table, id, rebuilt) {
                Some(bits) => constant(table, id, bits),
                None => rebuilt,
            };
            // Existing joins retain bounds computed during construction. A rewrite
            // must honor those carrier promises as well as the logical result.
            let original = table.bounds[id];
            let replacement = table.bounds[result];
            self.rewritten.insert(
                id,
                if replacement.unsigned <= original.unsigned
                    && replacement.signed <= original.signed
                {
                    result
                } else {
                    id
                },
            );
        }
        *root = self.rewritten[root];
    }
}

fn constant(table: &mut ValueTable, original: usize, bits: u64) -> usize {
    let ty = table.values[original].ty;
    let constant = table.constant(ty, bits);
    if ty.bits() < 32
        && table.bounds[original].signed <= ty.bits()
        && bits & (1 << (ty.bits() - 1)) != 0
    {
        // A narrow signed carrier may contain ones above its logical width.
        // Preserve that representation for consumers whose lowering relies on it.
        table.intern(Value {
            ty,
            definition: ValueDefinition::Expression(Expression::SignExtend { input: constant }),
        })
    } else {
        constant
    }
}
