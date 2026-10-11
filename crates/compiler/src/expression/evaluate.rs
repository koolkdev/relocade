//! Literal evaluation preserves raw vector bits and scalar carrier rules.
use super::{Expression, LaneBinaryOp};
use crate::{floating, integer, literal::Literal, Type};

#[derive(Clone, Copy)]
pub(crate) struct TypedLiteral {
    pub(crate) ty: Type,
    pub(crate) value: Literal,
}

impl Expression<TypedLiteral> {
    pub(crate) fn carrier_result(&self, result: Type, component: usize) -> Option<Literal> {
        // Lane widths describe the operation, even when narrow scalar inputs or
        // results use an i32 carrier. Replacement truncates to the logical lane.
        if matches!(
            self,
            Self::VectorExtract { .. } | Self::VectorReplace { .. }
        ) {
            return self.constant_result(result, component);
        }
        let expression = match *self {
            Self::SignExtend { .. } | Self::BitCount { .. } => *self,
            _ => self.map(|input| TypedLiteral {
                ty: input.ty.carrier(),
                value: input.value.normalize(input.ty.carrier()),
            }),
        };
        expression.constant_result(result.carrier(), component)
    }

    pub(crate) fn constant_result(&self, result: Type, component: usize) -> Option<Literal> {
        match *self {
            Self::VectorBinary {
                operator,
                lane_type,
                left,
                right,
            } => {
                let mask = u128::from(lane_type.mask());
                let mut bits = 0_u128;
                for shift in (0..128).step_by(lane_type.bits() as usize) {
                    let left = (u128::from(left.value) >> shift) & mask;
                    let right = (u128::from(right.value) >> shift) & mask;
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
                            if integer::signed_value(lane_type, left as u64)
                                > integer::signed_value(lane_type, right as u64)
                            {
                                mask
                            } else {
                                0
                            }
                        }
                    };
                    bits |= (lane & mask) << shift;
                }
                return Some(bits.into());
            }
            Self::VectorExtract { input, lane } => {
                return Some(Literal::from(result.normalize(
                    (u128::from(input.value) >> (lane * result.bits())) as u64,
                )));
            }
            Self::VectorShift {
                operator,
                lane_type,
                value,
                count,
            } => {
                let count = count.value.scalar(count.ty)? as u32 % u32::from(lane_type.bits());
                let mut bits = 0_u128;
                for offset in (0..128).step_by(lane_type.bits() as usize) {
                    let lane = ((u128::from(value.value) >> offset) as u64) & lane_type.mask();
                    let lane = match operator {
                        integer::ShiftOp::Left => lane << count,
                        integer::ShiftOp::RightUnsigned => lane >> count,
                        integer::ShiftOp::RightSigned => {
                            (integer::signed_value(lane_type, lane) >> count) as u64
                        }
                    };
                    bits |= u128::from(lane & lane_type.mask()) << offset;
                }
                return Some(bits.into());
            }
            Self::VectorReplace {
                vector,
                value,
                lane,
            } => {
                let shift = lane * value.ty.bits();
                let mask = u128::from(value.ty.mask()) << shift;
                // Refolding can leave carry bits above a narrow input's logical width.
                let lane_bits = value.ty.normalize(value.value.scalar(value.ty)?);
                return Some(
                    ((u128::from(vector.value) & !mask) | (u128::from(lane_bits) << shift)).into(),
                );
            }
            _ => {}
        }
        if let Self::Select {
            condition,
            when_true,
            when_false,
        } = *self
        {
            return Some(
                if condition.value.scalar(condition.ty)? != 0 {
                    when_true.value
                } else {
                    when_false.value
                }
                .normalize(result),
            );
        }
        if let Self::Bitwise {
            operator,
            left,
            right,
        } = *self
        {
            return Some(operator.apply(left.value, right.value).normalize(result));
        }
        self.try_map(|input| {
            Ok::<_, ()>(ScalarLiteral {
                ty: input.ty,
                value: input.value.scalar(input.ty).ok_or(())?,
            })
        })
        .ok()?
        .constant_result(result, component)
        .map(Literal::from)
    }
}

#[derive(Clone, Copy)]
struct ScalarLiteral {
    ty: Type,
    value: u64,
}

impl Expression<ScalarLiteral> {
    fn constant_result(&self, result: Type, component: usize) -> Option<u64> {
        debug_assert!(component < self.result_types(result).len());
        let bits = match *self {
            Self::VectorBinary { .. }
            | Self::VectorShift { .. }
            | Self::VectorExtract { .. }
            | Self::VectorReplace { .. } => return None,
            Self::FloatBinary {
                operator,
                left,
                right,
            } => floating::binary(operator, left.value, right.value)?,
            Self::FloatUnary { operator, input } => floating::unary(operator, input.value),
            Self::FloatCompare {
                operator,
                left,
                right,
            } => u64::from(floating::compare(operator, left.value, right.value)),
            Self::Reinterpret { input } => input.value,
            Self::Binary {
                operator,
                left,
                right,
            } => integer::binary(left.ty, operator, left.value, right.value)?,
            Self::MultiplyWide {
                signed,
                left,
                right,
            } => {
                let product = if signed {
                    ((left.value as i64 as i128) * (right.value as i64 as i128)) as u128
                } else {
                    u128::from(left.value) * u128::from(right.value)
                };
                (product >> (component * 64)) as u64
            }
            Self::Compare {
                operator,
                left,
                right,
            } => u64::from(integer::compare(left.ty, operator, left.value, right.value)),
            Self::Shift {
                operator,
                value,
                count,
            } => integer::shift(value.ty, operator, value.value, count.value as u32),
            Self::Rotate {
                operator,
                value,
                count,
            } => integer::rotate(value.ty, operator, value.value, count.value as u32),
            // Selection and bitwise operations evaluate their complete encodings above.
            Self::Select { .. } | Self::Bitwise { .. } => return None,
            Self::BitCount { operator, input } => {
                integer::bit_count(input.ty, operator, input.value)
            }
            Self::SignExtend { input } => integer::signed_value(input.ty, input.value) as u64,
            Self::ZeroTest { input, nonzero } => u64::from((input.value != 0) == nonzero),
            Self::Convert { input } => input.value,
            Self::LowBits { input, bits } => input.value & integer::low_mask(bits),
        };
        Some(result.normalize(bits))
    }
}
