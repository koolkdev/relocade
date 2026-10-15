//! Strict floating-point operations, separate from integer algebra.

pub(super) mod vector;
pub(super) use vector::VectorConvertOp;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum Format {
    Binary32,
    Binary64,
}

impl Format {
    pub(super) fn bits(self) -> u32 {
        match self {
            Self::Binary32 => 32,
            Self::Binary64 => 64,
        }
    }
}

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
    DemoteBinary32,
    FromSignedInteger,
    FromUnsignedInteger,
    TruncateSaturatingI32,
}

pub(super) fn convert(operator: ConvertOp, input_type: crate::Type, bits: u64) -> Option<u64> {
    let value = match operator {
        ConvertOp::PromoteBinary32 => f64::from(f32::from_bits(bits as u32)),
        ConvertOp::DemoteBinary32 => {
            let value = f64::from_bits(bits) as f32;
            return (!value.is_nan()).then(|| u64::from(value.to_bits()));
        }
        ConvertOp::FromSignedInteger => crate::integer::signed_value(input_type, bits) as f64,
        ConvertOp::FromUnsignedInteger => input_type.normalize(bits) as f64,
        ConvertOp::TruncateSaturatingI32 => return Some(f64::from_bits(bits) as i32 as u32 as u64),
    };
    // Conversion may quiet or canonicalize a NaN. Preserve the destination
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

pub(super) fn binary(format: Format, operator: BinaryOp, left: u64, right: u64) -> Option<u64> {
    macro_rules! calculate {
        ($float:ty, $bits:ty) => {{
            let left = <$float>::from_bits(left as $bits);
            let right = <$float>::from_bits(right as $bits);
            let result = match operator {
                BinaryOp::Add => left + right,
                BinaryOp::Sub => left - right,
                BinaryOp::Mul => left * right,
                BinaryOp::Div => left / right,
            };
            (!result.is_nan()).then(|| result.to_bits() as u64)
        }};
    }
    // Wasm permits several NaN results. Let the destination engine choose its
    // arithmetic NaN instead of importing the compiler host's payload policy.
    match format {
        Format::Binary32 => calculate!(f32, u32),
        Format::Binary64 => calculate!(f64, u64),
    }
}

pub(super) fn unary(format: Format, operator: UnaryOp, bits: u64) -> Option<u64> {
    let sign = 1_u64 << (format.bits() - 1);
    // Sign operations preserve every NaN payload bit.
    match operator {
        UnaryOp::Abs => return Some(bits & !sign),
        UnaryOp::Neg => return Some(bits ^ sign),
        _ => {}
    }
    macro_rules! calculate {
        ($float:ty, $bits:ty) => {{
            let value = <$float>::from_bits(bits as $bits);
            let result = match operator {
                UnaryOp::Ceil => value.ceil(),
                UnaryOp::Floor => value.floor(),
                UnaryOp::Trunc => value.trunc(),
                UnaryOp::Nearest => value.round_ties_even(),
                UnaryOp::Sqrt => value.sqrt(),
                UnaryOp::Abs | UnaryOp::Neg => unreachable!("sign operations preserve raw bits"),
            };
            (!result.is_nan()).then(|| result.to_bits() as u64)
        }};
    }
    // Rounding and square root use the destination engine's arithmetic NaN.
    match format {
        Format::Binary32 => calculate!(f32, u32),
        Format::Binary64 => calculate!(f64, u64),
    }
}

pub(super) fn compare(format: Format, operator: CompareOp, left: u64, right: u64) -> bool {
    // Promotion is exact for ordered binary32 values. NaN payloads do not
    // participate in numerical comparisons.
    let value = |bits| match format {
        Format::Binary32 => f64::from(f32::from_bits(bits as u32)),
        Format::Binary64 => f64::from_bits(bits),
    };
    let left = value(left);
    let right = value(right);
    match operator {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Lt => left < right,
        CompareOp::Le => left <= right,
        CompareOp::Gt => left > right,
        CompareOp::Ge => left >= right,
    }
}
