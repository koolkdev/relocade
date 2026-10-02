//! Derive facts through expressions without changing the current path.

use super::{Bits, Facts, Range};
use crate::{
    body::{ValueDefinition, ValueTable},
    expression::Constant,
    integer::{low_mask, BinaryOp, CompareOp},
    Expression,
};

impl Facts {
    pub(super) fn bits(&self, table: &ValueTable, root: usize) -> Bits {
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
            if let ValueDefinition::Constant(bits) = value.definition {
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
            if let Expression::Compare {
                operator,
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
            let inputs = expression.map(|&input| cache[&input]);
            let mut bits = match (expression, inputs) {
                (Expression::Convert { input }, Expression::Convert { input: bits }) => {
                    let width = table.bounds[input].unsigned;
                    let mask = low_mask(width);
                    bits.union(Bits {
                        mask: value.ty.mask() & !mask,
                        value: 0,
                    })
                }
                (
                    Expression::LowBits { bits: width, .. },
                    Expression::LowBits { input: bits, .. },
                ) => {
                    let mask = low_mask(width);
                    bits.restrict(mask).union(Bits {
                        mask: value.ty.mask() & !mask,
                        value: 0,
                    })
                }
                (
                    _,
                    Expression::Binary {
                        operator: BinaryOp::And,
                        left: a,
                        right: b,
                    },
                ) => {
                    let zeros = (a.mask & !a.value) | (b.mask & !b.value);
                    let ones = a.value & b.value;
                    Bits {
                        mask: zeros | ones,
                        value: ones,
                    }
                }
                (
                    _,
                    Expression::Binary {
                        operator: BinaryOp::Or,
                        left: a,
                        right: b,
                    },
                ) => {
                    let ones = a.value | b.value;
                    Bits {
                        mask: (a.mask & !a.value & b.mask & !b.value) | ones,
                        value: ones,
                    }
                }
                (
                    _,
                    Expression::Binary {
                        operator: BinaryOp::Xor,
                        left: a,
                        right: b,
                    },
                ) => {
                    let mask = a.mask & b.mask;
                    Bits {
                        mask,
                        value: (a.value ^ b.value) & mask,
                    }
                }
                (_, Expression::ZeroTest { input, nonzero }) if input.value != 0 => Bits {
                    mask: 1,
                    value: u64::from(nonzero),
                },
                _ => Bits::default(),
            };
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
                bits = Bits {
                    mask: 1,
                    value: u64::from(result),
                };
            }
            if let Ok(constants) = expression.try_map(|&input| {
                let bits = cache[&input];
                let ty = table[input].ty;
                if bits.mask & ty.mask() != ty.mask() {
                    return Err(());
                }
                Ok(Constant {
                    ty,
                    bits: table.carrier_bits(input, bits.value),
                })
            }) {
                if let Some(result) = constants.constant_result(value.ty, result.component) {
                    bits = Bits {
                        mask: value.ty.mask(),
                        value: result,
                    };
                }
            }
            cache.insert(id, bits.union(known).restrict(value.ty.mask()));
        }
        cache[&root]
    }
}
