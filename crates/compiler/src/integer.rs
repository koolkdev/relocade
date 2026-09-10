use crate::{Type, Value, ValueKind};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum BinaryOp {
    Add,
    Sub,
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum ShiftOp {
    Left,
    RightUnsigned,
    RightSigned,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum CompareOp {
    Eq,
    Ne,
    LtUnsigned,
    GeUnsigned,
    LtSigned,
    GeSigned,
}

pub(super) fn signed_value(ty: Type, bits: u64) -> i64 {
    let shift = 64 - ty.bits();
    (bits << shift) as i64 >> shift
}

pub(super) fn shift_count(ty: Type, count: u32) -> u32 {
    count & if ty == Type::I64 { 63 } else { 31 }
}

pub(super) fn binary(operator: BinaryOp, left: u64, right: u64) -> u64 {
    match operator {
        BinaryOp::Add => left.wrapping_add(right),
        BinaryOp::Sub => left.wrapping_sub(right),
        BinaryOp::And => left & right,
        BinaryOp::Or => left | right,
        BinaryOp::Xor => left ^ right,
    }
}

pub(super) fn compare(ty: Type, operator: CompareOp, left: u64, right: u64) -> bool {
    match operator {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::LtUnsigned => left < right,
        CompareOp::GeUnsigned => left >= right,
        CompareOp::LtSigned => signed_value(ty, left) < signed_value(ty, right),
        CompareOp::GeSigned => signed_value(ty, left) >= signed_value(ty, right),
    }
}

pub(super) fn shift(ty: Type, operator: ShiftOp, value: u64, count: u32) -> u64 {
    let count = shift_count(ty, count);
    match operator {
        ShiftOp::Left => value.wrapping_shl(count),
        ShiftOp::RightUnsigned => value >> count,
        ShiftOp::RightSigned => (signed_value(ty, value) >> count) as u64,
    }
}

pub(super) fn popcnt(value: u64) -> u64 {
    u64::from(value.count_ones())
}

/// A conservative bound on the nonzero bits in the emitted integer, including
/// upper bits that the logical type does not observe.
pub(super) fn unsigned_bits(value: Value, values: &[Value], inputs: &[u8]) -> u8 {
    let carrier_bits = if value.ty == Type::I64 { 64 } else { 32 };
    match value.kind {
        ValueKind::JoinResult { .. } => unreachable!("join bounds come from its yielding arms"),
        ValueKind::Constant(bits) => (64 - bits.leading_zeros()) as u8,
        ValueKind::Parameter(_) | ValueKind::Load { .. } | ValueKind::CallResult { .. } => {
            value.ty.bits()
        }
        ValueKind::Binary(operator, a, b) => match operator {
            BinaryOp::Add => inputs[a].max(inputs[b]).saturating_add(1).min(carrier_bits),
            // Underflow can set every carrier bit, including above a narrow type.
            BinaryOp::Sub => carrier_bits,
            BinaryOp::And => inputs[a].min(inputs[b]),
            BinaryOp::Or | BinaryOp::Xor => inputs[a].max(inputs[b]),
        },
        ValueKind::Shift {
            operator,
            value: input,
            count,
        } => match values[count].kind {
            ValueKind::Constant(bits) => {
                let count = shift_count(value.ty, bits as u32) as u8;
                match operator {
                    ShiftOp::Left => inputs[input].saturating_add(count).min(carrier_bits),
                    ShiftOp::RightUnsigned => inputs[input].saturating_sub(count),
                    ShiftOp::RightSigned => carrier_bits,
                }
            }
            _ => match operator {
                ShiftOp::Left => carrier_bits,
                ShiftOp::RightUnsigned => inputs[input],
                ShiftOp::RightSigned => carrier_bits,
            },
        },
        ValueKind::SignExtend(_) => carrier_bits,
        ValueKind::Popcnt(input) => (u8::BITS - inputs[input].leading_zeros()) as u8,
        ValueKind::Select {
            when_true,
            when_false,
            ..
        } => inputs[when_true].max(inputs[when_false]),
        ValueKind::Normalize(input) => inputs[input].min(value.ty.bits()),
        ValueKind::Convert(input) => inputs[input].min(carrier_bits),
        ValueKind::Compare(..) | ValueKind::ZeroTest { .. } => 1,
    }
}
