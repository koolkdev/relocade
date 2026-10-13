//! Literal evaluation preserves raw vector bits and scalar carrier rules.
use super::Expression;
use crate::expression::{
    BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, TernaryOp as Ternary,
    UnaryOp as Unary,
};

mod vector;
use crate::{floating, integer, literal::Literal, Type};

#[derive(Clone, Copy)]
pub(crate) struct TypedLiteral {
    pub(crate) ty: Type,
    pub(crate) value: Literal,
}

impl Expression<TypedLiteral> {
    pub(crate) fn carrier_result(&self, result: Type, component: usize) -> Option<Literal> {
        // Extraction's result type determines its lane width. Other width-sensitive
        // operations carry their interpretation in the opcode across substitutions.
        if matches!(
            self,
            Self::Unary {
                operator: Unary::ExtractLane { .. },
                ..
            }
        ) {
            return self.constant_result(result, component);
        }
        let expression = self.map(|input| TypedLiteral {
            ty: input.ty.carrier(),
            value: input.value.normalize(input.ty.carrier()),
        });
        expression.constant_result(result.carrier(), component)
    }

    pub(crate) fn constant_result(&self, result: Type, component: usize) -> Option<Literal> {
        debug_assert!(component < self.result_types(result).len());
        match *self {
            Self::Unary { operator, input } => unary(operator, input, result),
            Self::Binary {
                operator,
                left,
                right,
            } => binary(operator, left, right, result),
            Self::Shift {
                operator,
                value,
                count,
            } => {
                let count = count.value.scalar(count.ty)? as u32;
                let bits = match operator {
                    Shift::Lanes {
                        operator,
                        lane_type,
                    } => {
                        return Some(
                            Literal::from(vector::shift(
                                operator,
                                lane_type,
                                value.value.into(),
                                count,
                            ))
                            .normalize(result),
                        )
                    }
                    Shift::Scalar(operator) => {
                        integer::shift(value.ty, operator, value.value.scalar(value.ty)?, count)
                    }
                    Shift::Rotate(operator) => {
                        integer::rotate(value.ty, operator, value.value.scalar(value.ty)?, count)
                    }
                };
                Some(result.normalize(bits).into())
            }
            Self::Ternary {
                operator,
                first,
                second,
                third,
            } => ternary(operator, first, second, third, result),
            Self::MultiplyWide {
                signed,
                left,
                right,
            } => {
                let left = left.value.scalar(left.ty)?;
                let right = right.value.scalar(right.ty)?;
                let product = if signed {
                    ((left as i64 as i128) * (right as i64 as i128)) as u128
                } else {
                    u128::from(left) * u128::from(right)
                };
                Some(
                    result
                        .normalize((product >> (component * 64)) as u64)
                        .into(),
                )
            }
        }
    }
}

fn ternary(
    operator: Ternary,
    first: TypedLiteral,
    second: TypedLiteral,
    third: TypedLiteral,
    result: Type,
) -> Option<Literal> {
    let bits = match operator {
        Ternary::Select => {
            if third.value.scalar(third.ty)? != 0 {
                first.value
            } else {
                second.value
            }
        }
        Ternary::Bitselect => {
            let mask = u128::from(third.value);
            Literal::from((u128::from(first.value) & mask) | (u128::from(second.value) & !mask))
        }
        Ternary::Shuffle { input_type } => {
            let left = u128::from(first.value).to_le_bytes();
            let right = u128::from(second.value).to_le_bytes();
            let width = input_type.bits() / 8;
            let indices = u128::from(third.value).to_le_bytes();
            let bytes = indices.map(|index| {
                if index < width {
                    left[usize::from(index)]
                } else if index < width * 2 {
                    right[usize::from(index - width)]
                } else {
                    0
                }
            });
            Literal::from(u128::from_le_bytes(bytes))
        }
    };
    Some(bits.normalize(result))
}

fn unary(operator: Unary, input: TypedLiteral, result: Type) -> Option<Literal> {
    let bits = match operator {
        Unary::PairwiseAddUnsigned { lane_type } => {
            return Some(
                Literal::from(vector::pairwise_add_unsigned(lane_type, input.value.into()))
                    .normalize(result),
            );
        }
        Unary::Splat { lane_type } => {
            let lane = u128::from(lane_type.normalize(input.value.scalar(input.ty)?));
            let bits = (0..128)
                .step_by(usize::from(lane_type.bits()))
                .fold(0, |bits, offset| bits | (lane << offset));
            return Some(Literal::from(bits).normalize(result));
        }
        Unary::Reinterpret if !result.is_scalar() => {
            let low = u128::from(input.value.scalar(input.ty)?);
            return Some(Literal::from(low | (low << 64)).normalize(result));
        }
        Unary::Reinterpret if !input.ty.is_scalar() => u128::from(input.value) as u64,
        Unary::Convert if !input.ty.is_scalar() => return Some(input.value.normalize(result)),
        Unary::Bitmask { lane_type } => {
            let bits = u128::from(input.value);
            let mask = (0..128 / lane_type.bits()).fold(0_u32, |mask, lane| {
                mask | ((((bits >> ((lane + 1) * lane_type.bits() - 1)) & 1) as u32) << lane)
            });
            u64::from(mask)
        }
        Unary::ExtractLane { lane } => (u128::from(input.value) >> (lane * result.bits())) as u64,
        Unary::Float(operator) => floating::unary(operator, input.value.scalar(input.ty)?),
        Unary::BitCount {
            operator,
            input_type,
        } => integer::bit_count(
            input_type,
            operator,
            input_type.normalize(input.value.scalar(input.ty)?),
        ),
        Unary::SignExtend { input_type } => {
            integer::signed_value(input_type, input.value.scalar(input.ty)?) as u64
        }
        Unary::ZeroTest { nonzero } => u64::from((input.value.scalar(input.ty)? != 0) == nonzero),
        Unary::Convert | Unary::Reinterpret => input.value.scalar(input.ty)?,
        Unary::LowBits { bits } => input.value.scalar(input.ty)? & integer::low_mask(bits),
    };
    Some(result.normalize(bits).into())
}

fn binary(
    operator: Binary,
    left: TypedLiteral,
    right: TypedLiteral,
    result: Type,
) -> Option<Literal> {
    let bits = match operator {
        Binary::Swizzle { input_type } => {
            let bytes = u128::from(left.value).to_le_bytes();
            let indices = u128::from(right.value).to_le_bytes();
            let bytes = &bytes[..usize::from(input_type.bits() / 8)];
            return Some(
                Literal::from(u128::from_le_bytes(
                    indices.map(|i| bytes.get(usize::from(i)).copied().unwrap_or(0)),
                ))
                .normalize(result),
            );
        }
        Binary::Bitwise(operator) => {
            return Some(operator.apply(left.value, right.value).normalize(result))
        }
        Binary::Vector {
            operator,
            lane_type,
        } => {
            return Some(
                Literal::from(vector::binary(
                    operator,
                    lane_type,
                    left.value.into(),
                    right.value.into(),
                ))
                .normalize(result),
            )
        }
        Binary::Narrow {
            signed,
            result_lane,
        } => {
            return Some(
                Literal::from(vector::narrow(
                    signed,
                    result_lane,
                    left.value.into(),
                    right.value.into(),
                ))
                .normalize(result),
            )
        }
        Binary::ShuffleImmediate(lanes) => {
            return Some(
                Literal::from(vector::shuffle(
                    left.value.into(),
                    right.value.into(),
                    lanes.indices(),
                ))
                .normalize(result),
            )
        }
        Binary::ReplaceLane { lane_type, lane } => {
            let shift = lane * lane_type.bits();
            let mask = u128::from(lane_type.mask()) << shift;
            // Refolding can retain carry bits above the scalar's logical lane width.
            let lane_bits = lane_type.normalize(right.value.scalar(right.ty)?);
            return Some(
                Literal::from((u128::from(left.value) & !mask) | (u128::from(lane_bits) << shift))
                    .normalize(result),
            );
        }
        Binary::Integer(operator) => integer::binary(
            left.ty,
            operator,
            left.value.scalar(left.ty)?,
            right.value.scalar(right.ty)?,
        )?,
        Binary::Float(operator) => floating::binary(
            operator,
            left.value.scalar(left.ty)?,
            right.value.scalar(right.ty)?,
        )?,
        Binary::Compare(Compare::Integer(operator)) => u64::from(integer::compare(
            left.ty,
            operator,
            left.value.scalar(left.ty)?,
            right.value.scalar(right.ty)?,
        )),
        Binary::Compare(Compare::Float(operator)) => u64::from(floating::compare(
            operator,
            left.value.scalar(left.ty)?,
            right.value.scalar(right.ty)?,
        )),
    };
    Some(result.normalize(bits).into())
}
