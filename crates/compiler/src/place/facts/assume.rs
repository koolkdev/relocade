//! Learn facts implied by choosing one edge of a conditional branch.

use super::{Bits, ScalarFacts};
use crate::expression::{
    BinaryOp as Binary, CompareOp as Compare, TernaryOp as Ternary, UnaryOp as Unary,
};
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::{low_mask, CompareOp},
    Expression, Type,
};

impl ScalarFacts {
    pub(super) fn assume(&mut self, table: &ValueTable, condition: usize, truth: bool) {
        let mut pending = vec![(
            condition,
            Bits {
                mask: 1,
                value: u64::from(truth),
            },
        )];
        while let Some((id, bits)) = pending.pop() {
            let value = table.values[id];
            let bits = bits.restrict(value.ty.mask());
            if bits.mask == 0 {
                continue;
            }
            // Stored assumptions can decide this request without walking its
            // expression history again after an inference-cache invalidation.
            if self
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
                    Unary::Convert => pending.push((input, bits)),
                    Unary::LowBits { bits: width } => {
                        pending.push((input, bits.restrict(low_mask(width))))
                    }
                    Unary::ZeroTest { nonzero } if bits.mask & 1 != 0 => {
                        let is_zero = (bits.value & 1 != 0) != nonzero;
                        if is_zero {
                            pending.push((
                                input,
                                Bits {
                                    mask: table.values[input].ty.mask(),
                                    value: 0,
                                },
                            ));
                        } else if table.values[input].ty == Type::I1
                            || table.bounds[input].unsigned <= 1
                        {
                            pending.push((
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
                        if let Some(first) =
                            self.comparisons
                                .assume(table, operator, left, right, bits.value != 0)
                        {
                            self.invalidate_from(first);
                        }
                        self.assume_comparison(table, operator, left, right, bits.value != 0);
                        if matches!(operator, CompareOp::Eq | CompareOp::Ne)
                            && bits.mask & 1 != 0
                            && (bits.value & 1 != 0) == (operator == CompareOp::Eq)
                        {
                            pending.push((left, self.bits(table, right)));
                            pending.push((right, self.bits(table, left)));
                        }
                    }
                    Binary::Bitwise(BitwiseOp::Or) => {
                        let zeros = Bits {
                            mask: bits.mask & !bits.value,
                            value: 0,
                        };
                        pending.push((left, zeros));
                        pending.push((right, zeros));
                    }
                    Binary::Bitwise(BitwiseOp::And) => {
                        let ones = Bits {
                            mask: bits.value,
                            value: bits.value,
                        };
                        pending.push((left, ones));
                        pending.push((right, ones));
                        // A known mask exposes the same bits of the other operand.
                        for (input, other) in [(left, right), (right, left)] {
                            let other = self.bits(table, other);
                            pending.push((input, bits.restrict(other.value)));
                        }
                    }
                    _ => {}
                },
                Expression::Ternary {
                    operator: Ternary::Select,
                    first: when_true,
                    second: when_false,
                    third: condition,
                } => {
                    let truth = self
                        .constant(table, condition)
                        .map(|value| value != 0)
                        .or_else(|| self.bits(table, when_true).conflicts(bits).then_some(false))
                        .or_else(|| self.bits(table, when_false).conflicts(bits).then_some(true));
                    if let Some(truth) = truth {
                        pending.push((
                            condition,
                            Bits {
                                mask: 1,
                                value: u64::from(truth),
                            },
                        ));
                        pending.push((if truth { when_true } else { when_false }, bits));
                    }
                }
                _ => {}
            }
        }
    }
}
