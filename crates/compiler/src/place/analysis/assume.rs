//! Learn facts implied by choosing one edge of a conditional branch.

use super::{Bits, ValueAnalysis};
use crate::expression::{BinaryOp as Binary, CompareOp as Compare, UnaryOp as Unary};
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::{low_mask, CompareOp},
    Expression, Type,
};

mod vectors;

#[derive(Default)]
struct Pending {
    scalar: Vec<(usize, Bits)>,
    vector: Vec<(usize, Bits<u128>)>,
}

impl ValueAnalysis {
    pub(super) fn assume(&mut self, table: &ValueTable, condition: usize, truth: bool) {
        let mut pending = Pending {
            scalar: vec![(
                condition,
                Bits {
                    mask: 1,
                    value: u64::from(truth),
                },
            )],
            ..Pending::default()
        };
        loop {
            let Some((id, bits)) = pending.scalar.pop() else {
                let Some((id, bits)) = pending.vector.pop() else {
                    break;
                };
                self.assume_vector(table, id, bits, &mut pending);
                continue;
            };
            let value = table.values[id];
            let bits = bits.restrict(value.ty.mask());
            if bits.mask == 0 {
                continue;
            }
            // Previously learned observations can decide this request without
            // walking its expression again while this batch propagates.
            if self
                .context
                .known
                .get(&id)
                .is_some_and(|known| known.conflicts(bits) || bits.mask & !known.mask == 0)
            {
                continue;
            }
            let previous = self.bits(table, id);
            // Contradictory facts describe an unreachable arm. Keeping the old
            // facts is sufficient: its constant controlling branch is folded away.
            if previous.conflicts(bits) || bits.mask & !previous.mask == 0 {
                continue;
            }
            let bits = previous.union(bits);
            self.record(id, bits);
            let ValueDefinition::Expression(expression) = value.definition else {
                continue;
            };
            match expression {
                Expression::Unary { operator, input } => match operator {
                    Unary::Convert | Unary::Reinterpret => pending.scalar.push((input, bits)),
                    Unary::ExtractLane { lane } => {
                        let shift = lane * value.ty.bits();
                        pending.vector.push((
                            input,
                            Bits {
                                mask: u128::from(bits.mask) << shift,
                                value: u128::from(bits.value) << shift,
                            },
                        ));
                    }
                    Unary::LowBits { bits: width } => {
                        pending.scalar.push((input, bits.restrict(low_mask(width))))
                    }
                    Unary::ZeroTest { nonzero } if bits.mask & 1 != 0 => {
                        let is_zero = (bits.value & 1 != 0) != nonzero;
                        if is_zero {
                            pending.scalar.push((
                                input,
                                Bits {
                                    mask: table.values[input].ty.mask(),
                                    value: 0,
                                },
                            ));
                        } else if table.values[input].ty == Type::I1
                            || table.bounds[input].unsigned <= 1
                        {
                            pending.scalar.push((
                                input,
                                Bits {
                                    mask: table.values[input].ty.mask(),
                                    value: 1,
                                },
                            ));
                        } else {
                            self.assume_nonzero(table, input);
                        }
                    }
                    _ => {}
                },
                Expression::Binary {
                    operator,
                    left,
                    right,
                } => match operator {
                    Binary::Compare(Compare::Integer(operator)) => {
                        if let Some(first) = self.context.comparisons.assume(
                            table,
                            operator,
                            left,
                            right,
                            bits.value != 0,
                        ) {
                            self.invalidate_from(first);
                        }
                        self.assume_comparison(table, operator, left, right, bits.value != 0);
                        if matches!(operator, CompareOp::Eq | CompareOp::Ne)
                            && bits.mask & 1 != 0
                            && (bits.value & 1 != 0) == (operator == CompareOp::Eq)
                        {
                            pending.scalar.push((left, self.bits(table, right)));
                            pending.scalar.push((right, self.bits(table, left)));
                        }
                    }
                    Binary::Bitwise(operator @ (BitwiseOp::And | BitwiseOp::Or)) => {
                        let (a, b) = if operator == BitwiseOp::And {
                            (self.bits(table, left), self.bits(table, right))
                        } else {
                            (Bits::default(), Bits::default())
                        };
                        let [a, b] = bits.input_constraints(operator, a, b);
                        pending.scalar.push((left, a));
                        pending.scalar.push((right, b));
                    }
                    _ => {}
                },
                Expression::Select {
                    condition,
                    when_true,
                    when_false,
                } => {
                    let truth = self
                        .constant(table, condition)
                        .map(|value| value != 0)
                        .or_else(|| self.bits(table, when_true).conflicts(bits).then_some(false))
                        .or_else(|| self.bits(table, when_false).conflicts(bits).then_some(true));
                    if let Some(truth) = truth {
                        pending.scalar.push((
                            condition,
                            Bits {
                                mask: 1,
                                value: u64::from(truth),
                            },
                        ));
                        pending
                            .scalar
                            .push((if truth { when_true } else { when_false }, bits));
                    }
                }
                _ => {}
            }
        }
    }
}
