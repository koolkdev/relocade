//! Conservative widths of physical encodings, including unused logical upper bits.
//! These describe complete encodings, not floating-point magnitude or vector lanes.
//! Literal bounds describe their physical bits, including vector encodings.

use crate::expression::{BinaryOp as Binary, ShiftOp as Shift, UnaryOp as Unary};
use crate::{
    bitwise::BitwiseOp,
    body::{BlockItem, Value, ValueDefinition},
    integer::{shift_count, BinaryOp, BitCountOp, ShiftOp},
    Expression, Type,
};

#[derive(Clone, Copy)]
pub(crate) struct BitBounds {
    /// An unsigned value fits in this many bits; all higher bits are zero.
    pub(crate) unsigned: u8,
    /// A signed value fits in this many bits; all higher bits repeat its sign.
    pub(crate) signed: u8,
}

impl BitBounds {
    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            unsigned: self.unsigned.max(other.unsigned),
            signed: self.signed.max(other.signed),
        }
    }

    pub(crate) fn for_value(value: Value, values: &[Value], inputs: &[Self]) -> Self {
        let carrier = value.ty.carrier().bits();
        let unsigned = unsigned_bits(value, values, inputs);
        // A zero-extended value needs one more bit for a nonnegative sign. At
        // full carrier width no narrower signed representation is established.
        let fallback = unsigned.saturating_add(1).min(carrier);
        let signed = match value.definition {
            ValueDefinition::Literal(literal) => {
                literal.scalar(value.ty).map_or(fallback, |bits| {
                    let signed = if carrier == 64 {
                        bits as i64
                    } else {
                        bits as u32 as i32 as i64
                    };
                    let magnitude = if signed < 0 { !signed } else { signed } as u64;
                    (65 - magnitude.leading_zeros()) as u8
                })
            }
            ValueDefinition::Expression(Expression::Unary { operator, input }) => match operator {
                Unary::SignExtend { input_type } => inputs[input].signed.min(input_type.bits()),
                Unary::Convert => {
                    if value.ty == Type::I64 && values[input].ty != Type::I64 {
                        // Widening this conversion is unsigned, including negative
                        // i32 carriers whose upper half becomes zero in i64.
                        inputs[input].unsigned.saturating_add(1).min(carrier)
                    } else {
                        inputs[input].signed.min(carrier)
                    }
                }
                _ => fallback,
            },
            ValueDefinition::Expression(Expression::Binary {
                operator: Binary::Integer(BinaryOp::Mul),
                left,
                right,
            }) => inputs[left]
                .signed
                .saturating_add(inputs[right].signed)
                .min(carrier),
            _ => fallback,
        };
        Self { unsigned, signed }
    }
}

fn unsigned_bits(value: Value, values: &[Value], inputs: &[BitBounds]) -> u8 {
    let carrier = value.ty.carrier().bits();
    let expression = match value.definition {
        // Backedges may carry wider intermediate bits than their initial values.
        // Loop edges preserve those bits just like ordinary result joins.
        ValueDefinition::Parameter { .. } => return carrier,
        ValueDefinition::Literal(literal) => return literal.unsigned_bits(),
        ValueDefinition::Result {
            producer: BlockItem::Effect(_),
            ..
        } => return value.ty.bits(),
        ValueDefinition::Result {
            producer: BlockItem::Evaluate(_),
            ..
        } => return carrier,
        ValueDefinition::Expression(expression) => expression,
    };
    match expression {
        Expression::Unary { operator, input } => match operator {
            Unary::BitCount {
                operator,
                input_type,
            } => {
                let maximum = match operator {
                    BitCountOp::Ones => inputs[input].unsigned,
                    BitCountOp::LeadingZeros | BitCountOp::TrailingZeros => input_type.bits(),
                };
                (u8::BITS - maximum.leading_zeros()) as u8
            }
            Unary::LowBits { bits } => inputs[input].unsigned.min(bits),
            // I64 -> V64 lowers to a splat. The unused upper half is not zero.
            Unary::Reinterpret if value.ty == Type::V64 => carrier,
            Unary::Convert | Unary::Reinterpret => inputs[input].unsigned.min(carrier),
            Unary::ZeroTest { .. } => 1,
            Unary::Bitmask { lane_type } => 128 / lane_type.bits(),
            _ => carrier,
        },
        Expression::Binary {
            operator,
            left,
            right,
        } => match operator {
            Binary::Integer(operator) => match operator {
                BinaryOp::Add => inputs[left]
                    .unsigned
                    .max(inputs[right].unsigned)
                    .saturating_add(1)
                    .min(carrier),
                // Underflow can set every carrier bit, including above a narrow type.
                BinaryOp::Sub => carrier,
                BinaryOp::Mul => inputs[left]
                    .unsigned
                    .saturating_add(inputs[right].unsigned)
                    .min(carrier),
                BinaryOp::DivUnsigned | BinaryOp::RemUnsigned => value.ty.bits(),
                BinaryOp::DivSigned | BinaryOp::RemSigned => carrier,
            },
            Binary::Bitwise(operator) => match operator {
                BitwiseOp::And => inputs[left].unsigned.min(inputs[right].unsigned),
                BitwiseOp::Or | BitwiseOp::Xor => inputs[left].unsigned.max(inputs[right].unsigned),
            },
            Binary::Compare(_) => 1,
            _ => carrier,
        },
        Expression::Shift {
            operator,
            value: input,
            count,
        } => match operator {
            Shift::Scalar(operator) => match values[count].scalar_literal() {
                Some(bits) => {
                    let count = shift_count(value.ty, bits as u32) as u8;
                    match operator {
                        ShiftOp::Left => inputs[input].unsigned.saturating_add(count).min(carrier),
                        ShiftOp::RightUnsigned => inputs[input].unsigned.saturating_sub(count),
                        ShiftOp::RightSigned => carrier,
                    }
                }
                _ => match operator {
                    ShiftOp::Left => carrier,
                    ShiftOp::RightUnsigned => inputs[input].unsigned,
                    ShiftOp::RightSigned => carrier,
                },
            },
            Shift::Rotate(_) | Shift::Lanes { .. } => carrier,
        },
        Expression::Select {
            when_true,
            when_false,
            ..
        } => inputs[when_true].unsigned.max(inputs[when_false].unsigned),
        Expression::MultiplyWide { .. } => carrier,
    }
}
