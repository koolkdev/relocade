//! Transfer known encoding bits through scalar and packed operations.

use super::KnownBits;
use crate::{
    bitwise::BitwiseOp, body::ValueTable, expression::TypedLiteral, integer::low_mask, Expression,
    Type,
};

pub(super) fn infer(
    table: &ValueTable,
    ty: Type,
    expression: Expression<usize>,
    inputs: Expression<KnownBits>,
) -> KnownBits {
    match (expression, inputs) {
        (Expression::Convert { input }, Expression::Convert { input: bits }) => {
            let width = table.bounds[input].unsigned;
            let mask = u128::from(low_mask(width));
            bits.union(KnownBits {
                mask: ty.bit_mask() & !mask,
                value: 0,
            })
        }
        (Expression::LowBits { bits: width, .. }, Expression::LowBits { input: bits, .. }) => {
            let mask = u128::from(low_mask(width));
            bits.restrict(mask).union(KnownBits {
                mask: ty.bit_mask() & !mask,
                value: 0,
            })
        }
        (
            _,
            Expression::Bitwise {
                operator: BitwiseOp::And,
                left: a,
                right: b,
            },
        ) => {
            let zeros = (a.mask & !a.value) | (b.mask & !b.value);
            let ones = a.value & b.value;
            KnownBits {
                mask: zeros | ones,
                value: ones,
            }
        }
        (
            _,
            Expression::Bitwise {
                operator: BitwiseOp::Or,
                left: a,
                right: b,
            },
        ) => {
            let ones = a.value | b.value;
            KnownBits {
                mask: (a.mask & !a.value & b.mask & !b.value) | ones,
                value: ones,
            }
        }
        (
            _,
            Expression::Bitwise {
                operator: BitwiseOp::Xor,
                left: a,
                right: b,
            },
        ) => {
            let mask = a.mask & b.mask;
            KnownBits {
                mask,
                value: (a.value ^ b.value) & mask,
            }
        }
        (
            _,
            Expression::Select {
                when_true,
                when_false,
                ..
            },
        ) => when_true.common(when_false),
        (
            Expression::VectorReplace { value, lane, .. },
            Expression::VectorReplace {
                vector,
                value: bits,
                ..
            },
        ) => {
            let shift = lane * table[value].ty.bits();
            let mask = table[value].ty.bit_mask() << shift;
            vector.restrict(!mask).union(
                KnownBits {
                    mask: bits.mask << shift,
                    value: bits.value << shift,
                }
                .restrict(mask),
            )
        }
        (Expression::VectorExtract { lane, .. }, Expression::VectorExtract { input, .. }) => {
            let shift = lane * ty.bits();
            KnownBits {
                mask: input.mask >> shift,
                value: input.value >> shift,
            }
        }
        (
            Expression::VectorBinary { lane_type, .. },
            Expression::VectorBinary { left, right, .. },
        ) => {
            let lane_mask = lane_type.bit_mask();
            let mut mask = 0;
            for shift in (0..128).step_by(lane_type.bits().into()) {
                let lane = lane_mask << shift;
                if left.mask & right.mask & lane == lane {
                    mask |= lane;
                }
            }
            // Only complete known lanes contribute. Reuse literal lane
            // semantics; the arbitrary unknown bits are masked away.
            let literals = inputs.map(|bits| TypedLiteral {
                ty: Type::V128,
                value: bits.value.into(),
            });
            KnownBits {
                mask,
                value: u128::from(literals.constant_result(Type::V128, 0).unwrap()) & mask,
            }
        }
        (_, Expression::ZeroTest { input, nonzero }) if input.value != 0 => KnownBits {
            mask: 1,
            value: u128::from(nonzero),
        },
        _ => KnownBits::default(),
    }
}
