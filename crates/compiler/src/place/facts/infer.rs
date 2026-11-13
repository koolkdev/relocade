//! Derive facts through expressions without changing the current path.

use super::{Bits, ScalarFacts};
use crate::expression::{BinaryOp as Binary, CompareOp as Compare, TernaryOp as Ternary};
use crate::{body::ValueTable, Expression};

mod expression;

impl ScalarFacts {
    /// Known scalar bits; non-scalar values contribute no knowledge.
    pub(super) fn bits(&self, table: &ValueTable, root: usize) -> Bits {
        let mut cache = self.computed.borrow_mut();
        if let Some(&bits) = cache.get(&root) {
            return bits;
        }
        let mut pending = self.pending.borrow_mut();
        pending.push((root, false));
        while let Some((id, ready)) = pending.pop() {
            if cache.contains_key(&id) {
                continue;
            }
            let value = table[id];
            if !value.ty.is_scalar() {
                cache.insert(id, Bits::default());
                continue;
            }
            let known = self.known_bits(table, id);
            if let Some(bits) = value.scalar_literal() {
                cache.insert(
                    id,
                    Bits {
                        mask: value.ty.mask(),
                        value: value.ty.normalize(bits),
                    },
                );
                continue;
            }
            if known.mask == value.ty.mask() {
                cache.insert(id, known);
                continue;
            }
            let Some(result) = table.expression(id) else {
                cache.insert(id, known);
                continue;
            };
            let expression = result.expression;
            if let Expression::Binary {
                operator: Binary::Compare(Compare::Integer(operator)),
                left,
                right,
            } = expression
            {
                if let Some(truth) = self.comparisons.get(table, operator, left, right) {
                    cache.insert(
                        id,
                        Bits {
                            mask: 1,
                            value: u64::from(truth),
                        },
                    );
                    continue;
                }
            }
            // Resolve the selector first so known paths do not walk discarded
            // state histories merely to rediscover the chosen value.
            if let Expression::Ternary {
                operator: Ternary::Select,
                first: when_true,
                second: when_false,
                third: condition,
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
                        cache.insert(id, bits.union(known).restrict(value.ty.mask()));
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
            let bits = self.expression_bits(table, id, |input| cache[&input]);
            cache.insert(id, bits.union(known).restrict(value.ty.mask()));
        }
        cache[&root]
    }
}
