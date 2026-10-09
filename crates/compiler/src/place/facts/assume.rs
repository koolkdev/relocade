//! Learn facts implied by choosing one edge of a conditional branch.

use super::{Bits, Facts};
use crate::{
    bitwise::BitwiseOp,
    body::{ValueDefinition, ValueTable},
    integer::{low_mask, CompareOp},
    Expression, Type,
};

impl Facts {
    pub(in crate::place) fn assume(&mut self, table: &ValueTable, condition: usize, truth: bool) {
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
            if let Expression::Compare {
                operator,
                left,
                right,
            } = expression
            {
                if let Some(first) =
                    self.comparisons
                        .assume(table, operator, left, right, bits.value != 0)
                {
                    self.invalidate_from(first);
                }
                self.assume_comparison(table, operator, left, right, bits.value != 0);
            }
            match expression {
                Expression::Convert { input } => pending.push((input, bits)),
                Expression::LowBits { input, bits: width } => {
                    pending.push((input, bits.restrict(low_mask(width))))
                }
                Expression::Bitwise {
                    operator: BitwiseOp::Or,
                    left: a,
                    right: b,
                } => {
                    let zeros = Bits {
                        mask: bits.mask & !bits.value,
                        value: 0,
                    };
                    pending.push((a, zeros));
                    pending.push((b, zeros));
                }
                Expression::Bitwise {
                    operator: BitwiseOp::And,
                    left: a,
                    right: b,
                } => {
                    let ones = Bits {
                        mask: bits.value,
                        value: bits.value,
                    };
                    pending.push((a, ones));
                    pending.push((b, ones));
                    // A known mask exposes the same bits of the other operand.
                    for (input, other) in [(a, b), (b, a)] {
                        let other = self.bits(table, other);
                        pending.push((input, bits.restrict(other.value)));
                    }
                }
                Expression::ZeroTest { input, nonzero } if bits.mask & 1 != 0 => {
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
                Expression::Compare {
                    operator: operator @ (CompareOp::Eq | CompareOp::Ne),
                    left: a,
                    right: b,
                } if bits.mask & 1 != 0 && (bits.value & 1 != 0) == (operator == CompareOp::Eq) => {
                    pending.push((a, self.bits(table, b)));
                    pending.push((b, self.bits(table, a)));
                }
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
