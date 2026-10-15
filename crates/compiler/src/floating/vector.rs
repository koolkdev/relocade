//! Packed literal evaluation uses the same strict arithmetic as scalar values.

use super::{BinaryOp, CompareOp, Format, UnaryOp};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum VectorConvertOp {
    PromoteLowBinary32,
    DemoteBinary64Zero,
    FromSignedI32 { format: Format },
    TruncateSaturatingI32 { format: Format },
}

fn map_lanes(format: Format, mut calculate: impl FnMut(u32) -> Option<u64>) -> Option<u128> {
    let mut bits = 0_u128;
    for offset in (0..128).step_by(format.bits() as usize) {
        bits |= u128::from(calculate(offset)?) << offset;
    }
    Some(bits)
}

pub(crate) fn binary(format: Format, operator: BinaryOp, left: u128, right: u128) -> Option<u128> {
    map_lanes(format, |offset| {
        super::binary(
            format,
            operator,
            (left >> offset) as u64,
            (right >> offset) as u64,
        )
    })
}

pub(crate) fn unary(format: Format, operator: UnaryOp, input: u128) -> Option<u128> {
    map_lanes(format, |offset| {
        let lane = (input >> offset) as u64 & (u64::MAX >> (64 - format.bits()));
        super::unary(format, operator, lane)
    })
}

pub(crate) fn compare(format: Format, operator: CompareOp, left: u128, right: u128) -> u128 {
    let mask = u64::MAX >> (64 - format.bits());
    map_lanes(format, |offset| {
        Some(
            if super::compare(
                format,
                operator,
                (left >> offset) as u64,
                (right >> offset) as u64,
            ) {
                mask
            } else {
                0
            },
        )
    })
    .unwrap()
}

pub(crate) fn convert(operator: VectorConvertOp, input: u128) -> Option<u128> {
    let mut result = 0_u128;
    let (count, input_bits, output_bits) = match operator {
        VectorConvertOp::PromoteLowBinary32 => (2, 32, 64),
        VectorConvertOp::DemoteBinary64Zero => (2, 64, 32),
        VectorConvertOp::FromSignedI32 { format } => (128 / format.bits(), 32, format.bits()),
        VectorConvertOp::TruncateSaturatingI32 { format } => {
            (128 / format.bits(), format.bits(), 32)
        }
    };
    for lane in 0..count {
        let bits = (input >> (lane * input_bits)) as u64;
        let bits = match operator {
            VectorConvertOp::PromoteLowBinary32 => {
                super::convert(super::ConvertOp::PromoteBinary32, crate::Type::I32, bits)?
            }
            VectorConvertOp::DemoteBinary64Zero => {
                super::convert(super::ConvertOp::DemoteBinary32, crate::Type::F64, bits)?
            }
            VectorConvertOp::FromSignedI32 {
                format: Format::Binary32,
            } => u64::from((bits as i32 as f32).to_bits()),
            VectorConvertOp::FromSignedI32 {
                format: Format::Binary64,
            } => (bits as i32 as f64).to_bits(),
            VectorConvertOp::TruncateSaturatingI32 {
                format: Format::Binary32,
            } => f32::from_bits(bits as u32) as i32 as u32 as u64,
            VectorConvertOp::TruncateSaturatingI32 {
                format: Format::Binary64,
            } => f64::from_bits(bits) as i32 as u32 as u64,
        };
        result |= u128::from(bits) << (lane * output_bits);
    }
    Some(result)
}
