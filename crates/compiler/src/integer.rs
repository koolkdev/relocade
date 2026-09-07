use crate::{Type, Value, ValueKind};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum BinaryOp {
    Add,
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum ShiftOp {
    Left,
    Right,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum CompareOp {
    Eq,
    Ne,
    Lt,
    Ge,
}

pub(super) fn shift_count(ty: Type, count: u32) -> u32 {
    count & if ty == Type::I64 { 63 } else { 31 }
}

/// A conservative bound on the nonzero bits in the emitted integer, including
/// upper bits that the logical type does not observe.
pub(super) fn unsigned_bits(value: Value, inputs: &[u8]) -> u8 {
    let carrier_bits = if value.ty == Type::I64 { 64 } else { 32 };
    match value.kind {
        ValueKind::JoinResult { .. } => unreachable!("join bounds come from its yielding arms"),
        ValueKind::Constant(bits) => (64 - bits.leading_zeros()) as u8,
        ValueKind::Parameter(_) | ValueKind::Load { .. } | ValueKind::CallResult { .. } => {
            value.ty.bits()
        }
        ValueKind::Binary(operator, a, b) => match operator {
            BinaryOp::Add => inputs[a].max(inputs[b]).saturating_add(1).min(carrier_bits),
            BinaryOp::And => inputs[a].min(inputs[b]),
            BinaryOp::Or | BinaryOp::Xor => inputs[a].max(inputs[b]),
        },
        ValueKind::Shift(operator, input, count) => match operator {
            ShiftOp::Left => inputs[input]
                .saturating_add(shift_count(value.ty, count) as u8)
                .min(carrier_bits),
            ShiftOp::Right => inputs[input].saturating_sub(shift_count(value.ty, count) as u8),
        },
        ValueKind::Normalize(input) => inputs[input].min(value.ty.bits()),
        ValueKind::Convert(input) => inputs[input].min(carrier_bits),
        ValueKind::Compare(..) | ValueKind::ZeroTest { .. } => 1,
    }
}
