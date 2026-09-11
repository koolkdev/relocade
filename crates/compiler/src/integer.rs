use crate::Type;

mod bounds;

pub(super) use bounds::BitBounds;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum BinaryOp {
    Add,
    Sub,
    Mul,
    DivUnsigned,
    DivSigned,
    RemUnsigned,
    RemSigned,
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
pub(super) enum RotateOp {
    Left,
    Right,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum BitCountOp {
    Ones,
    LeadingZeros,
    TrailingZeros,
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

pub(super) fn rotate_count(ty: Type, count: u32) -> u32 {
    count & u32::from(ty.bits() - 1)
}

pub(super) fn binary(ty: Type, operator: BinaryOp, left: u64, right: u64) -> Option<u64> {
    Some(match operator {
        BinaryOp::Add => left.wrapping_add(right),
        BinaryOp::Sub => left.wrapping_sub(right),
        BinaryOp::Mul => left.wrapping_mul(right),
        BinaryOp::DivUnsigned => left.checked_div(right)?,
        BinaryOp::RemUnsigned => left.checked_rem(right)?,
        BinaryOp::DivSigned => {
            let left = signed_value(ty, left);
            let right = signed_value(ty, right);
            // Leave operations without a native numeric result in the expression.
            // Narrow inputs divide in i32, then retain their logical low bits.
            if ty == Type::I32 && left == i64::from(i32::MIN) && right == -1 {
                return None;
            }
            left.checked_div(right)? as u64
        }
        BinaryOp::RemSigned => {
            let left = signed_value(ty, left);
            let right = signed_value(ty, right);
            if left == i64::MIN && right == -1 {
                0
            } else {
                left.checked_rem(right)? as u64
            }
        }
        BinaryOp::And => left & right,
        BinaryOp::Or => left | right,
        BinaryOp::Xor => left ^ right,
    })
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

pub(super) fn rotate(ty: Type, operator: RotateOp, value: u64, count: u32) -> u64 {
    let count = rotate_count(ty, count);
    if count == 0 {
        return value;
    }
    let remaining = u32::from(ty.bits()) - count;
    match operator {
        RotateOp::Left => (value << count) | (value >> remaining),
        RotateOp::Right => (value >> count) | (value << remaining),
    }
}

pub(super) fn bit_count(ty: Type, operator: BitCountOp, value: u64) -> u64 {
    let width = u32::from(ty.bits());
    u64::from(match operator {
        BitCountOp::Ones => value.count_ones(),
        BitCountOp::LeadingZeros => value.leading_zeros() - (64 - width),
        BitCountOp::TrailingZeros => value.trailing_zeros().min(width),
    })
}
