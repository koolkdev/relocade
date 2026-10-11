//! Literal SIMD operations use full vector encodings and logical lane widths.

use crate::{
    expression::LaneBinaryOp,
    integer::{self, ShiftOp},
    Type,
};

pub(super) fn binary(operator: LaneBinaryOp, lane_type: Type, left: u128, right: u128) -> u128 {
    match operator {
        LaneBinaryOp::WideningMulLowSigned => return widening_mul(lane_type, left, right),
        LaneBinaryOp::WideningMulHighSigned => {
            return widening_mul(lane_type, left >> 64, right >> 64)
        }
        LaneBinaryOp::DotI16Signed => return dot_i16_pairs(left, right),
        _ => {}
    }
    let mask = lane_type.mask();
    let mut bits = 0_u128;
    for offset in (0..128).step_by(lane_type.bits() as usize) {
        let left = (left >> offset) as u64 & mask;
        let right = (right >> offset) as u64 & mask;
        let lane = match operator {
            LaneBinaryOp::Add => left.wrapping_add(right),
            LaneBinaryOp::Sub => left.wrapping_sub(right),
            LaneBinaryOp::Mul => left.wrapping_mul(right),
            LaneBinaryOp::Eq => {
                if left == right {
                    mask
                } else {
                    0
                }
            }
            LaneBinaryOp::GtSigned => {
                if integer::signed_value(lane_type, left) > integer::signed_value(lane_type, right)
                {
                    mask
                } else {
                    0
                }
            }
            LaneBinaryOp::AddSatUnsigned => (left + right).min(mask),
            LaneBinaryOp::SubSatUnsigned => left.saturating_sub(right),
            LaneBinaryOp::AddSatSigned | LaneBinaryOp::SubSatSigned => {
                let left = integer::signed_value(lane_type, left);
                let right = integer::signed_value(lane_type, right);
                let result = if matches!(operator, LaneBinaryOp::AddSatSigned) {
                    left + right
                } else {
                    left - right
                };
                let limit = 1_i64 << (lane_type.bits() - 1);
                result.clamp(-limit, limit - 1) as u64
            }
            LaneBinaryOp::WideningMulLowSigned
            | LaneBinaryOp::WideningMulHighSigned
            | LaneBinaryOp::DotI16Signed => {
                unreachable!("width-changing operations were evaluated above")
            }
        };
        bits |= u128::from(lane & mask) << offset;
    }
    bits
}

fn widening_mul(lane_type: Type, left: u128, right: u128) -> u128 {
    let result_width = lane_type.bits() * 2;
    let mask = u64::MAX >> (64 - result_width);
    let mut bits = 0_u128;
    for lane in 0..64 / lane_type.bits() {
        let offset = lane * lane_type.bits();
        let left = integer::signed_value(lane_type, (left >> offset) as u64);
        let right = integer::signed_value(lane_type, (right >> offset) as u64);
        bits |= u128::from((left * right) as u64 & mask) << (lane * result_width);
    }
    bits
}

fn dot_i16_pairs(left: u128, right: u128) -> u128 {
    let mut bits = 0_u128;
    for offset in (0..128).step_by(32) {
        let product = |offset| {
            integer::signed_value(Type::I16, (left >> offset) as u64)
                * integer::signed_value(Type::I16, (right >> offset) as u64)
        };
        let sum = product(offset) + product(offset + 16);
        bits |= u128::from(sum as u32) << offset;
    }
    bits
}

pub(super) fn shift(operator: ShiftOp, lane_type: Type, value: u128, count: u32) -> u128 {
    let count = count % u32::from(lane_type.bits());
    let mut bits = 0_u128;
    for offset in (0..128).step_by(lane_type.bits() as usize) {
        let lane = ((value >> offset) as u64) & lane_type.mask();
        let lane = match operator {
            ShiftOp::Left => lane << count,
            ShiftOp::RightUnsigned => lane >> count,
            ShiftOp::RightSigned => (integer::signed_value(lane_type, lane) >> count) as u64,
        };
        bits |= u128::from(lane & lane_type.mask()) << offset;
    }
    bits
}

pub(super) fn shuffle(left: u128, right: u128, lanes: [u8; 16]) -> u128 {
    let inputs = [left.to_le_bytes(), right.to_le_bytes()];
    u128::from_le_bytes(lanes.map(|lane| inputs[usize::from(lane / 16)][usize::from(lane % 16)]))
}

pub(super) fn narrow(signed: bool, result_lane: Type, left: u128, right: u128) -> u128 {
    let input_lane = match result_lane {
        Type::I8 => Type::I16,
        Type::I16 => Type::I32,
        _ => unreachable!("narrowing produces byte or word lanes"),
    };
    let (minimum, maximum) = if signed {
        let limit = 1_i64 << (result_lane.bits() - 1);
        (-limit, limit - 1)
    } else {
        (0, result_lane.mask() as i64)
    };
    let mut bits = 0_u128;
    for (half, input) in [left, right].into_iter().enumerate() {
        for lane in 0..128 / input_lane.bits() {
            let value =
                integer::signed_value(input_lane, (input >> (lane * input_lane.bits())) as u64);
            let result = value.clamp(minimum, maximum) as u64 & result_lane.mask();
            bits |= u128::from(result) << (half * 64 + usize::from(lane * result_lane.bits()));
        }
    }
    bits
}
