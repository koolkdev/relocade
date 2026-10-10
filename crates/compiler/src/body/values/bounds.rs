//! Conservative widths of physical encodings, including unused logical upper bits.
//! For floating values these describe encoding bits, not numerical magnitude.

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
            ValueDefinition::Literal(bits) => {
                let signed = if carrier == 64 {
                    bits as i64
                } else {
                    bits as u32 as i32 as i64
                };
                let magnitude = if signed < 0 { !signed } else { signed } as u64;
                (65 - magnitude.leading_zeros()) as u8
            }
            ValueDefinition::Expression(expression) => match expression {
                Expression::SignExtend { input } => {
                    inputs[input].signed.min(values[input].ty.bits())
                }
                Expression::Binary {
                    operator: BinaryOp::Mul,
                    left: a,
                    right: b,
                } => inputs[a]
                    .signed
                    .saturating_add(inputs[b].signed)
                    .min(carrier),
                Expression::Convert { input } => {
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
            _ => fallback,
        };
        Self { unsigned, signed }
    }
}

fn unsigned_bits(value: Value, values: &[Value], inputs: &[BitBounds]) -> u8 {
    let carrier = value.ty.carrier().bits();
    match value.definition {
        // Backedges may carry wider intermediate bits than their initial values.
        // Loop edges preserve those bits just like ordinary result joins.
        ValueDefinition::Parameter { .. } => carrier,
        ValueDefinition::Literal(bits) => (64 - bits.leading_zeros()) as u8,
        ValueDefinition::Result {
            producer: BlockItem::Effect(_),
            ..
        } => value.ty.bits(),
        ValueDefinition::Result {
            producer: BlockItem::Evaluate(_),
            ..
        } => carrier,
        ValueDefinition::Expression(expression) => match expression {
            Expression::Binary {
                operator,
                left: a,
                right: b,
            } => match operator {
                BinaryOp::Add => inputs[a]
                    .unsigned
                    .max(inputs[b].unsigned)
                    .saturating_add(1)
                    .min(carrier),
                // Underflow can set every carrier bit, including above a narrow type.
                BinaryOp::Sub => carrier,
                BinaryOp::Mul => inputs[a]
                    .unsigned
                    .saturating_add(inputs[b].unsigned)
                    .min(carrier),
                BinaryOp::DivUnsigned | BinaryOp::RemUnsigned => value.ty.bits(),
                BinaryOp::DivSigned | BinaryOp::RemSigned => carrier,
            },
            Expression::Bitwise {
                operator,
                left: a,
                right: b,
            } => match operator {
                BitwiseOp::And => inputs[a].unsigned.min(inputs[b].unsigned),
                BitwiseOp::Or | BitwiseOp::Xor => inputs[a].unsigned.max(inputs[b].unsigned),
            },
            Expression::Shift {
                operator,
                value: input,
                count,
            } => match values[count].scalar_literal() {
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
            Expression::Rotate { .. }
            | Expression::SignExtend { .. }
            | Expression::MultiplyWide { .. }
            | Expression::FloatBinary { .. }
            | Expression::FloatUnary { .. } => carrier,
            Expression::BitCount { operator, input } => {
                let maximum = match operator {
                    BitCountOp::Ones => inputs[input].unsigned,
                    BitCountOp::LeadingZeros | BitCountOp::TrailingZeros => value.ty.bits(),
                };
                (u8::BITS - maximum.leading_zeros()) as u8
            }
            Expression::Select {
                when_true,
                when_false,
                ..
            } => inputs[when_true].unsigned.max(inputs[when_false].unsigned),
            Expression::LowBits { input, bits } => inputs[input].unsigned.min(bits),
            Expression::Convert { input } | Expression::Reinterpret { input } => {
                inputs[input].unsigned.min(carrier)
            }
            Expression::Compare { .. }
            | Expression::FloatCompare { .. }
            | Expression::ZeroTest { .. } => 1,
        },
    }
}
