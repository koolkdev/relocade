//! Input encodings required to preserve an observed part of one result.

use crate::{
    body::ValueTable,
    expression::LaneBinaryOp,
    integer::{low_mask, BinaryOp},
    Expression,
};

/// Preserve carrier bits unless the operation itself defines a narrower observation.
/// Unknown dependencies deliberately retain the complete input encoding.
pub(in crate::place) fn inputs(
    values: &ValueTable,
    result: usize,
    observed: u128,
) -> Expression<u128> {
    let expression = values.expression(result).unwrap().expression;
    expression.map(|&input| {
        let needed = match expression {
            Expression::Bitwise { .. }
            | Expression::Reinterpret { .. }
            | Expression::Convert { .. } => observed,
            Expression::LowBits { bits, .. } => observed & u128::from(low_mask(bits)),
            Expression::Binary {
                operator: BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul,
                ..
            } => prefix(observed),
            Expression::Select { condition, .. } => {
                if input == condition {
                    u128::MAX
                } else {
                    observed
                }
            }
            Expression::VectorExtract { lane, .. } => {
                (observed & values[result].ty.bit_mask()) << (lane * values[result].ty.bits())
            }
            Expression::VectorReplace { value, lane, .. } => {
                let shift = lane * values[value].ty.bits();
                let lane_mask = values[value].ty.bit_mask();
                if input == value {
                    (observed >> shift) & lane_mask
                } else {
                    observed & !(lane_mask << shift)
                }
            }
            Expression::VectorBinary {
                operator,
                lane_type,
                ..
            } => {
                let mut needed = 0;
                for shift in (0..128).step_by(lane_type.bits().into()) {
                    let lane = (observed >> shift) & lane_type.bit_mask();
                    let bits = match operator {
                        LaneBinaryOp::Add | LaneBinaryOp::Sub => prefix(lane),
                        LaneBinaryOp::Eq | LaneBinaryOp::GtSigned => {
                            if lane == 0 {
                                0
                            } else {
                                lane_type.bit_mask()
                            }
                        }
                    };
                    needed |= bits << shift;
                }
                needed
            }
            _ => u128::MAX,
        };
        needed & values[input].ty.carrier().bit_mask()
    })
}

fn prefix(mask: u128) -> u128 {
    u128::MAX.checked_shr(mask.leading_zeros()).unwrap_or(0)
}
