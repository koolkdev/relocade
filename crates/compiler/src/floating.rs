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

pub(super) fn unary(operator: UnaryOp, bits: u64) -> u64 {
    // Unlike arithmetic, these operations preserve every NaN payload bit.
    match operator {
        UnaryOp::Abs => bits & !(1 << 63),
        UnaryOp::Neg => bits ^ (1 << 63),
    }
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
