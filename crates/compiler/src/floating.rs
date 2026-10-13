//! Strict scalar floating-point operations, separate from integer algebra.

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum UnaryOp {
    Abs,
    Neg,
    Ceil,
    Floor,
    Trunc,
    Nearest,
    Sqrt,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum ConvertOp {
    PromoteBinary32,
    FromSignedInteger,
    FromUnsignedInteger,
}

pub(super) fn convert(operator: ConvertOp, input_type: crate::Type, bits: u64) -> Option<u64> {
    let value = match operator {
        ConvertOp::PromoteBinary32 => f64::from(f32::from_bits(bits as u32)),
        ConvertOp::FromSignedInteger => crate::integer::signed_value(input_type, bits) as f64,
        ConvertOp::FromUnsignedInteger => input_type.normalize(bits) as f64,
    };
    // Promotion may quiet or canonicalize a NaN. Preserve the destination
    // engine's permitted result instead of folding the host's payload choice.
    (!value.is_nan()).then(|| value.to_bits())
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

pub(super) fn binary(operator: BinaryOp, left: u64, right: u64) -> Option<u64> {
    let left = f64::from_bits(left);
    let right = f64::from_bits(right);
    let result = match operator {
        BinaryOp::Add => left + right,
        BinaryOp::Sub => left - right,
        BinaryOp::Mul => left * right,
        BinaryOp::Div => left / right,
    };
    // Wasm permits several NaN results. Let the destination engine choose its
    // arithmetic NaN instead of importing the compiler host's payload policy.
    (!result.is_nan()).then(|| result.to_bits())
}

pub(super) fn unary(operator: UnaryOp, bits: u64) -> Option<u64> {
    let value = f64::from_bits(bits);
    let result = match operator {
        // Sign operations preserve every NaN payload bit.
        UnaryOp::Abs => return Some(bits & !(1 << 63)),
        UnaryOp::Neg => return Some(bits ^ (1 << 63)),
        UnaryOp::Ceil => value.ceil(),
        UnaryOp::Floor => value.floor(),
        UnaryOp::Trunc => value.trunc(),
        UnaryOp::Nearest => value.round_ties_even(),
        UnaryOp::Sqrt => value.sqrt(),
    };
    // Rounding and square root use the destination engine's arithmetic NaN.
    (!result.is_nan()).then(|| result.to_bits())
}

pub(super) fn compare(operator: CompareOp, left: u64, right: u64) -> bool {
    let left = f64::from_bits(left);
    let right = f64::from_bits(right);
    match operator {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Lt => left < right,
        CompareOp::Le => left <= right,
        CompareOp::Gt => left > right,
        CompareOp::Ge => left >= right,
    }
}
