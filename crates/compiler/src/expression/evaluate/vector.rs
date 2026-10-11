//! Literal SIMD operations use full vector encodings and logical lane widths.

use crate::{
    expression::LaneBinaryOp,
    integer::{self, ShiftOp},
    Type,
};

pub(super) fn binary(operator: LaneBinaryOp, lane_type: Type, left: u128, right: u128) -> u128 {
    let mask = lane_type.mask();
    let mut bits = 0_u128;
    for offset in (0..128).step_by(lane_type.bits() as usize) {
        let left = (left >> offset) as u64 & mask;
        let right = (right >> offset) as u64 & mask;
        let lane = match operator {
            LaneBinaryOp::Add => left.wrapping_add(right),
            LaneBinaryOp::Sub => left.wrapping_sub(right),
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
        };
        bits |= u128::from(lane & mask) << offset;
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
