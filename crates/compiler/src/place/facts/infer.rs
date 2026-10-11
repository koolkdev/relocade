//! Derive facts through expressions without changing the current path.

use super::{KnownBits, Range, ValueFacts};
use crate::{
    body::{ValueDefinition, ValueTable},
    expression::TypedLiteral,
    integer::CompareOp,
    Expression,
};

impl ValueFacts {
    /// Infer logical encoding bits through the same graph for every value type.
    pub(super) fn bits(&self, table: &ValueTable, root: usize) -> KnownBits {
        let mut cache = self.computed.borrow_mut();
        if let Some(&bits) = cache.get(&root) {
            return bits;
        }
        let mut pending = vec![(root, false)];
        while let Some((id, ready)) = pending.pop() {
            if cache.contains_key(&id) {
                continue;
            }
            let value = table[id];
            let known = self.known.get(&id).copied().unwrap_or_default();
            if let ValueDefinition::Literal(bits) = value.definition {
                cache.insert(
                    id,
                    KnownBits {
                        mask: value.ty.bit_mask(),
                        value: u128::from(bits) & value.ty.bit_mask(),
                    },
                );
                continue;
            }
            if known.mask == value.ty.bit_mask() {
                cache.insert(id, known);
                continue;
            }
            let Some(result) = table.expression(id) else {
                cache.insert(id, known);
                continue;
            };
            let expression = result.expression;
            if let Expression::Compare {
                operator,
                left,
                right,
            } = expression
            {
                if let Some(truth) = self.comparisons.get(table, operator, left, right) {
                    cache.insert(
                        id,
                        KnownBits {
                            mask: 1,
                            value: u128::from(truth),
                        },
                    );
                    continue;
                }
            }
            // Resolve the selector first so known paths do not walk discarded
            // state histories merely to rediscover the chosen value.
            if let Expression::Select {
                condition,
                when_true,
                when_false,
            } = expression
            {
                let Some(condition_bits) = cache.get(&condition) else {
                    pending.push((id, false));
                    pending.push((condition, false));
                    continue;
                };
                if condition_bits.mask & 1 != 0 {
                    let input = if condition_bits.value & 1 != 0 {
                        when_true
                    } else {
                        when_false
                    };
                    if let Some(&bits) = cache.get(&input) {
                        cache.insert(id, bits.union(known).restrict(value.ty.bit_mask()));
                    } else {
                        pending.push((id, false));
                        pending.push((input, false));
                    }
                    continue;
                }
            }
            if !ready {
                pending.push((id, true));
                pending.extend(expression.inputs().map(|&input| (input, false)));
                continue;
            }
            let inputs = expression.map(|&input| cache[&input]);
            let mut bits = super::encoding::infer(table, value.ty, expression, inputs);
            let comparison = match expression {
                Expression::Compare {
                    operator,
                    left,
                    right,
                } => self
                    .range(table, left, cache[&left])
                    .zip(self.range(table, right, cache[&right]))
                    .and_then(|(left, right)| left.compare(operator, right)),
                Expression::ZeroTest { input, nonzero } => {
                    self.range(table, input, cache[&input]).and_then(|range| {
                        range.compare(
                            if nonzero {
                                CompareOp::Ne
                            } else {
                                CompareOp::Eq
                            },
                            Range {
                                minimum: 0,
                                maximum: 0,
                            },
                        )
                    })
                }
                _ => None,
            };
            if let Some(result) = comparison {
                bits = KnownBits {
                    mask: 1,
                    value: u128::from(result),
                };
            }
            if let Ok(constants) = expression.try_map(|&input| {
                let bits = cache[&input];
                let ty = table[input].ty;
                if bits.mask & ty.bit_mask() != ty.bit_mask() {
                    return Err(());
                }
                Ok(TypedLiteral {
                    ty,
                    value: if ty.is_scalar() {
                        table.carrier_bits(input, bits.value as u64).into()
                    } else {
                        bits.value.into()
                    },
                })
            }) {
                if let Some(result) = constants.constant_result(value.ty, result.component) {
                    bits = KnownBits {
                        mask: value.ty.bit_mask(),
                        value: u128::from(result),
                    };
                }
            }
            cache.insert(id, bits.union(known).restrict(value.ty.bit_mask()));
        }
        cache[&root]
    }
}
