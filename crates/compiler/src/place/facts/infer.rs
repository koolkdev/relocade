//! Derive facts through expressions without changing the current path.

use rustc_hash::FxHashMap;

use super::{Bits, Range, ScalarFacts};
use crate::{
    bitwise::BitwiseOp,
    body::ValueTable,
    expression::TypedLiteral,
    integer::{low_mask, CompareOp},
    Expression,
};

/// Inferred facts and reusable storage for walking their dependencies.
#[derive(Clone, Default, Eq, PartialEq)]
pub(super) struct InferenceCache {
    bits: FxHashMap<usize, Bits>,
    // Every query drains the worklist; keep its capacity for the next one.
    pending: Vec<(usize, bool)>,
}

impl InferenceCache {
    pub(super) fn clear(&mut self) {
        self.bits.clear();
    }

    pub(super) fn invalidate_from(&mut self, id: usize) {
        self.bits.retain(|&input, _| input < id);
    }
}

impl ScalarFacts {
    /// Known scalar bits; non-scalar values contribute no knowledge.
    pub(super) fn bits(&self, table: &ValueTable, root: usize) -> Bits {
        let mut computed = self.computed.borrow_mut();
        let InferenceCache {
            bits: cache,
            pending,
        } = &mut *computed;
        if let Some(&bits) = cache.get(&root) {
            return bits;
        }
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
            let known = self.known.get(&id).copied().unwrap_or_default();
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
                    Expression::Bitwise {
                        operator: BitwiseOp::And,
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
                    Expression::Bitwise {
                        operator: BitwiseOp::Or,
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
                    Expression::Bitwise {
                        operator: BitwiseOp::Xor,
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
                if !ty.is_scalar() {
                    return Err(());
                }
                if bits.mask & ty.mask() != ty.mask() {
                    return Err(());
                }
                Ok(TypedLiteral {
                    ty,
                    value: table.carrier_bits(input, bits.value).into(),
                })
            }) {
                if let Some(result) = constants
                    .constant_result(value.ty, result.component)
                    .and_then(|literal| literal.scalar(value.ty))
                {
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
