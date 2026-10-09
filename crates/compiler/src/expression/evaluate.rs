//! Literal evaluation follows logical widths and lowered carrier rules.

use super::Expression;
use crate::{floating, integer, Type};

#[derive(Clone, Copy)]
pub(crate) struct TypedLiteral {
    pub(crate) ty: Type,
    pub(crate) value: u64,
}

impl Expression<TypedLiteral> {
    /// Evaluate operations whose logical input normalization is already explicit.
    pub(crate) fn carrier_result(&self, result: Type, component: usize) -> Option<u64> {
        let expression = match *self {
            // These operations explicitly interpret the source's logical width.
            Self::SignExtend { .. } | Self::BitCount { .. } => *self,
            _ => self.map(|input| TypedLiteral {
                ty: input.ty.carrier(),
                value: input.ty.carrier().normalize(input.value),
            }),
        };
        expression.constant_result(result.carrier(), component)
    }

    pub(crate) fn constant_result(&self, result: Type, component: usize) -> Option<u64> {
        debug_assert!(component < self.result_types(result).len());
        let bits = match *self {
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
            Self::Select {
                condition,
                when_true,
                when_false,
            } => {
                if condition.value != 0 {
                    when_true.value
                } else {
                    when_false.value
                }
            }
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
