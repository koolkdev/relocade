//! Infer one expression from its input facts, without traversing the value graph.

use super::super::{Bits, Range, ScalarFacts};
use crate::expression::{
    BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, TernaryOp as Ternary,
    UnaryOp as Unary,
};
use crate::{
    bitwise::BitwiseOp,
    body::ValueTable,
    expression::TypedLiteral,
    integer::{low_mask, CompareOp},
    Expression,
};

impl ScalarFacts {
    pub(super) fn expression_bits(
        &self,
        table: &ValueTable,
        id: usize,
        input_bits: impl Fn(usize) -> Bits,
    ) -> Bits {
        let value = table[id];
        let result = table.expression(id).unwrap();
        let expression = result.expression;
        let mut bits = match expression {
            Expression::Unary { operator, input } => {
                let input_bits = input_bits(input);
                match operator {
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
            } => {
                let a = input_bits(left);
                let b = input_bits(right);
                match operator {
                    BitwiseOp::And => {
                        let zeros = (a.mask & !a.value) | (b.mask & !b.value);
                        let ones = a.value & b.value;
                        Bits {
                            mask: zeros | ones,
                            value: ones,
                        }
                    }
                    BitwiseOp::Or => {
                        let ones = a.value | b.value;
                        Bits {
                            mask: (a.mask & !a.value & b.mask & !b.value) | ones,
                            value: ones,
                        }
                    }
                    BitwiseOp::Xor => {
                        let mask = a.mask & b.mask;
                        Bits {
                            mask,
                            value: (a.value ^ b.value) & mask,
                        }
                    }
                }
            }
            Expression::Shift {
                operator: Shift::Scalar(operator),
                value: input,
                count,
            } => input_bits(input).shifted(table, input, operator, input_bits(count)),
            Expression::Ternary {
                operator: Ternary::Select | Ternary::Bitselect,
                first: when_true,
                second: when_false,
                ..
            } => input_bits(when_true).common(input_bits(when_false)),
            _ => Bits::default(),
        };
        let comparison = match expression {
            Expression::Binary {
                operator: Binary::Compare(Compare::Integer(operator)),
                left,
                right,
            } => self
                .range(table, left, input_bits(left))
                .zip(self.range(table, right, input_bits(right)))
                .and_then(|(left, right)| left.compare(operator, right)),
            Expression::Unary {
                operator: Unary::ZeroTest { nonzero },
                input,
            } => self
                .range(table, input, input_bits(input))
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
            let bits = input_bits(input);
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
        bits
    }
}
