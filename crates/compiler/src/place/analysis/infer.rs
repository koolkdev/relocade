//! Derive facts through expressions without changing the current path.

use super::{Bits, DerivedFacts, Range, ValueAnalysis};
use crate::expression::{
    BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, UnaryOp as Unary,
};
use crate::{
    body::ValueTable,
    expression::TypedLiteral,
    integer::{low_mask, CompareOp},
    Expression,
};

impl ValueAnalysis {
    /// Physical zeros supplement logical facts; a logical width never supplies
    /// them by itself. Stored literals already describe their complete carrier.
    pub(super) fn scalar_carrier_bits(&self, table: &ValueTable, id: usize) -> Bits {
        let mask = table[id].ty.carrier().mask();
        if let Some(value) = table[id].scalar_literal() {
            return Bits {
                mask,
                value: value & mask,
            };
        }
        self.bits(table, id).union(Bits {
            mask: mask & !low_mask(table.bounds[id].unsigned),
            value: 0,
        })
    }

    /// Scalar queries keep their cache entries and returned encodings at u64 width.
    pub(super) fn bits(&self, table: &ValueTable, root: usize) -> Bits {
        if !table[root].ty.is_scalar() {
            return Bits::default();
        }
        let mut derived = self.derived.borrow_mut();
        if let Some(&bits) = derived.bits.get(&root) {
            return bits;
        }
        self.infer(table, root, &mut derived);
        derived.bits[&root]
    }

    pub(super) fn vector_bits(&self, table: &ValueTable, root: usize) -> Bits<u128> {
        let mut derived = self.derived.borrow_mut();
        if let Some(&bits) = derived.vectors.as_ref().and_then(|bits| bits.get(&root)) {
            return bits;
        }
        self.infer(table, root, &mut derived);
        derived.vectors.as_ref().unwrap()[&root]
    }

    /// A single worklist crosses scalar/vector lane boundaries without nested queries.
    fn infer(&self, table: &ValueTable, root: usize, derived: &mut DerivedFacts) {
        let mut pending = self.pending.borrow_mut();
        Inference {
            analysis: self,
            table,
            derived,
            pending: &mut pending,
        }
        .run(root);
    }
}

mod vectors;

/// One query owns its derivation buffers while walking across encoding domains.
struct Inference<'a> {
    analysis: &'a ValueAnalysis,
    table: &'a ValueTable,
    derived: &'a mut DerivedFacts,
    pending: &'a mut Vec<(usize, bool)>,
}

impl Inference<'_> {
    fn run(&mut self, root: usize) {
        let table = self.table;
        self.pending.push((root, false));
        while let Some((id, ready)) = self.pending.pop() {
            if !table[id].ty.is_scalar() {
                self.vector(id, ready);
                continue;
            }
            let derived = &mut self.derived;
            let pending = &mut self.pending;
            let cache = &mut derived.bits;
            if cache.contains_key(&id) {
                continue;
            }
            let value = table[id];
            let known = self
                .analysis
                .context
                .known
                .get(&id)
                .copied()
                .unwrap_or_default();
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
                if let Some(truth) = self
                    .analysis
                    .context
                    .comparisons
                    .get(table, operator, left, right)
                {
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
            if let Expression::Unary {
                operator: Unary::ExtractLane { lane },
                input,
            } = expression
            {
                let input = derived.vectors.as_ref().unwrap()[&input];
                let shift = lane * value.ty.bits();
                let bits = Bits {
                    mask: (input.mask >> shift) as u64,
                    value: (input.value >> shift) as u64,
                };
                cache.insert(id, bits.union(known).restrict(value.ty.mask()));
                continue;
            }
            let mut bits = match expression {
                Expression::Unary { operator, input } => {
                    let input_bits = cache[&input];
                    match operator {
                        Unary::Reinterpret => input_bits,
                        Unary::Convert => {
                            let mask = low_mask(table.bounds[input].unsigned);
                            input_bits.union(Bits {
                                mask: value.ty.mask() & !mask,
                                value: 0,
                            })
                        }
                        Unary::LowBits { bits: width } => {
                            let mask = low_mask(width);
                            input_bits.restrict(mask).union(Bits {
                                mask: value.ty.mask() & !mask,
                                value: 0,
                            })
                        }
                        Unary::ZeroTest { nonzero } if input_bits.value != 0 => Bits {
                            mask: 1,
                            value: u64::from(nonzero),
                        },
                        _ => Bits::default(),
                    }
                }
                Expression::Binary {
                    operator: Binary::Bitwise(operator),
                    left,
                    right,
                } => cache[&left].bitwise(operator, cache[&right]),
                Expression::Shift {
                    operator: Shift::Scalar(operator),
                    value: input,
                    count,
                } => cache[&input].shifted(table, input, operator, cache[&count]),
                Expression::Select {
                    when_true,
                    when_false,
                    ..
                } => cache[&when_true].common(cache[&when_false]),
                _ => Bits::default(),
            };
            let comparison = match expression {
                Expression::Binary {
                    operator: Binary::Compare(Compare::Integer(operator)),
                    left,
                    right,
                } => self
                    .analysis
                    .range(table, left, cache[&left])
                    .zip(self.analysis.range(table, right, cache[&right]))
                    .and_then(|(left, right)| left.compare(operator, right)),
                Expression::Unary {
                    operator: Unary::ZeroTest { nonzero },
                    input,
                } => self
                    .analysis
                    .range(table, input, cache[&input])
                    .and_then(|range| {
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
                    }),
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
                    value: table.carrier_bits(input, bits.value).ok_or(())?.into(),
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
    }
}
