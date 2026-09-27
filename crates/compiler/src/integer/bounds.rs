//! Conservative bit widths of emitted values, including unused logical upper bits.

use super::{shift_count, BinaryOp, BitCountOp, ShiftOp};
use crate::{
    body::{Value, ValueDefinition},
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
        let carrier = if value.ty == Type::I64 { 64 } else { 32 };
        let unsigned = unsigned_bits(value, values, inputs);
        // A zero-extended value needs one more bit for a nonnegative sign. At
        // full carrier width no narrower signed representation is established.
        let fallback = unsigned.saturating_add(1).min(carrier);
        let signed = match value.definition {
            ValueDefinition::Constant(bits) => {
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
    let carrier = if value.ty == Type::I64 { 64 } else { 32 };
    match value.definition {
        // Backedges may carry wider intermediate bits than their initial values.
        // Loop edges preserve those bits just like ordinary result joins.
        ValueDefinition::LoopInput { .. } => carrier,
        ValueDefinition::JoinResult { .. } => {
            unreachable!("join bounds come from its yielding arms")
        }
        ValueDefinition::Constant(bits) => (64 - bits.leading_zeros()) as u8,
        ValueDefinition::Parameter(_)
        | ValueDefinition::Load { .. }
        | ValueDefinition::OperationResult { .. } => value.ty.bits(),
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
                BinaryOp::And => inputs[a].unsigned.min(inputs[b].unsigned),
                BinaryOp::Or | BinaryOp::Xor => inputs[a].unsigned.max(inputs[b].unsigned),
            },
            Expression::Shift {
                operator,
                value: input,
                count,
            } => match values[count].definition {
                ValueDefinition::Constant(bits) => {
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
            Expression::Rotate { .. } | Expression::SignExtend { .. } => carrier,
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
            Expression::Normalize { input } => inputs[input].unsigned.min(value.ty.bits()),
            Expression::Convert { input } => inputs[input].unsigned.min(carrier),
            Expression::Compare { .. } | Expression::ZeroTest { .. } => 1,
        },
    }
}
