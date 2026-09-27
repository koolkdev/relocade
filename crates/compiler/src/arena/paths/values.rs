//! Rebuild calculations at their uses; snapshots and effect results keep identity.

use super::{super::ValueArena, Path};
use crate::{Expression, Value, ValueDefinition};

impl Path {
    pub(super) fn value(&mut self, arena: &mut ValueArena, root: &mut usize) {
        let mut pending = vec![(*root, false)];
        while let Some((id, ready)) = pending.pop() {
            if self.rewritten.contains_key(&id) {
                continue;
            }
            let value = arena.values[id];
            if let Some(bits) = self.facts.constant(arena, id) {
                let result = constant(arena, id, bits);
                self.rewritten.insert(id, result);
                continue;
            }
            if let Some(&shared) = self.shared.get(&id) {
                // A known selection can use an input or snapshot directly.
                // Calculated arms retain the group's rewrite: exposing one
                // separately could make placement repeat it on the same path.
                let result = match arena.values[shared].definition {
                    ValueDefinition::Expression(Expression::Select {
                        condition,
                        when_true,
                        when_false,
                    }) => match self.facts.constant(arena, condition) {
                        Some(0) => when_false,
                        Some(_) => when_true,
                        None => shared,
                    },
                    _ => shared,
                };
                self.rewritten.insert(
                    id,
                    if matches!(
                        arena.values[result].definition,
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
            let rebuilt = match expression {
                Expression::Binary {
                    operator: op,
                    left: a,
                    right: b,
                } => {
                    if arena.values[a].ty == value.ty && arena.values[b].ty == value.ty {
                        arena.binary(op, a, b)
                    } else {
                        arena.intern(Value {
                            ty: value.ty,
                            definition: ValueDefinition::Expression(Expression::Binary {
                                operator: op,
                                left: a,
                                right: b,
                            }),
                        })
                    }
                }
                Expression::Compare {
                    operator: op,
                    left: a,
                    right: b,
                } => arena.compare(op, a, b),
                // These nodes already contain carrier conversions chosen during
                // construction. Preserve them instead of lowering logical inputs again.
                Expression::Shift { .. } | Expression::Convert { .. } => arena.intern(Value {
                    ty: value.ty,
                    definition: ValueDefinition::Expression(expression),
                }),
                Expression::Rotate {
                    operator,
                    value,
                    count,
                } => arena.rotate(operator, value, count),
                Expression::Select {
                    condition,
                    when_true,
                    when_false,
                } => arena.select(condition, when_true, when_false),
                Expression::Normalize { input } => arena.normalize(input),
                Expression::SignExtend { input } => arena.sign_extend(input, value.ty),
                Expression::BitCount { operator, input } => arena.bit_count(operator, input),
                Expression::ZeroTest { input, nonzero } => arena.zero_test(input, nonzero),
            };
            let result = match self.facts.refine(arena, id, rebuilt) {
                Some(bits) => constant(arena, id, bits),
                None => rebuilt,
            };
            // Existing joins retain bounds computed during construction. A rewrite
            // must honor those carrier promises as well as the logical result.
            let original = arena.bounds[id];
            let replacement = arena.bounds[result];
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

fn constant(arena: &mut ValueArena, original: usize, bits: u64) -> usize {
    let ty = arena.values[original].ty;
    let constant = arena.constant(ty, bits);
    if ty.bits() < 32
        && arena.bounds[original].signed <= ty.bits()
        && bits & (1 << (ty.bits() - 1)) != 0
    {
        // A narrow signed carrier may contain ones above its logical width.
        // Preserve that representation for consumers whose lowering relies on it.
        arena.intern(Value {
            ty,
            definition: ValueDefinition::Expression(Expression::SignExtend { input: constant }),
        })
    } else {
        constant
    }
}
