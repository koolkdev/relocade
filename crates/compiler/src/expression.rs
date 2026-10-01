//! Pure expressions shared by unbound values and function bodies.

mod fold;
pub(crate) use fold::{build, map_inputs, normalize};

#[cfg(test)]
mod tests;

use crate::{
    integer::{self, BinaryOp, BitCountOp, CompareOp, RotateOp, ShiftOp},
    Type,
};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum Expression<V> {
    Binary {
        operator: BinaryOp,
        left: V,
        right: V,
    },
    MultiplyWide {
        signed: bool,
        left: V,
        right: V,
    },
    Shift {
        operator: ShiftOp,
        value: V,
        count: V,
    },
    Rotate {
        operator: RotateOp,
        value: V,
        count: V,
    },
    Select {
        condition: V,
        when_true: V,
        when_false: V,
    },
    SignExtend {
        input: V,
    },
    BitCount {
        operator: BitCountOp,
        input: V,
    },
    Compare {
        operator: CompareOp,
        left: V,
        right: V,
    },
    ZeroTest {
        input: V,
        nonzero: bool,
    },
    Convert {
        input: V,
    },
    LowBits {
        input: V,
        bits: u8,
    },
}

impl<V> Expression<V> {
    /// Logical result types in their declared order. The caller supplies the
    /// scalar result type; operations with a fixed signature declare it here.
    pub(super) fn result_types(&self, scalar: Type) -> impl ExactSizeIterator<Item = Type> {
        let (types, count) = match self {
            Self::MultiplyWide { .. } => ([Type::I64; 2], 2),
            _ => ([scalar; 2], 1),
        };
        types.into_iter().take(count)
    }

    /// Inputs in execution and Wasm stack order: select's condition comes last.
    pub(super) fn inputs(&self) -> impl DoubleEndedIterator<Item = &V> {
        // Expressions have at most three inputs. Mapping defines their
        // order once, including for callers that only need to walk the inputs.
        let mut inputs = [None; 3];
        let mut count = 0;
        self.map(|input| {
            inputs[count] = Some(input);
            count += 1;
        });
        inputs.into_iter().flatten()
    }

    pub(super) fn map<'a, U>(&'a self, mut input: impl FnMut(&'a V) -> U) -> Expression<U> {
        self.try_map(|value| Ok::<_, std::convert::Infallible>(input(value)))
            .unwrap_or_else(|error| match error {})
    }

    /// Visit each input in Wasm stack order, stopping at the first error.
    pub(super) fn try_map<'a, U, E>(
        &'a self,
        mut input: impl FnMut(&'a V) -> Result<U, E>,
    ) -> Result<Expression<U>, E> {
        Ok(match self {
            Self::Binary {
                operator,
                left,
                right,
            } => Expression::Binary {
                operator: *operator,
                left: input(left)?,
                right: input(right)?,
            },
            Self::MultiplyWide {
                signed,
                left,
                right,
            } => Expression::MultiplyWide {
                signed: *signed,
                left: input(left)?,
                right: input(right)?,
            },
            Self::Shift {
                operator,
                value,
                count,
            } => Expression::Shift {
                operator: *operator,
                value: input(value)?,
                count: input(count)?,
            },
            Self::Rotate {
                operator,
                value,
                count,
            } => Expression::Rotate {
                operator: *operator,
                value: input(value)?,
                count: input(count)?,
            },
            Self::Select {
                condition,
                when_true,
                when_false,
            } => Expression::Select {
                when_true: input(when_true)?,
                when_false: input(when_false)?,
                condition: input(condition)?,
            },
            Self::SignExtend { input: value } => Expression::SignExtend {
                input: input(value)?,
            },
            Self::BitCount {
                operator,
                input: value,
            } => Expression::BitCount {
                operator: *operator,
                input: input(value)?,
            },
            Self::Compare {
                operator,
                left,
                right,
            } => Expression::Compare {
                operator: *operator,
                left: input(left)?,
                right: input(right)?,
            },
            Self::ZeroTest {
                input: value,
                nonzero,
            } => Expression::ZeroTest {
                input: input(value)?,
                nonzero: *nonzero,
            },
            Self::Convert { input: value } => Expression::Convert {
                input: input(value)?,
            },
            Self::LowBits { input: value, bits } => Expression::LowBits {
                input: input(value)?,
                bits: *bits,
            },
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Constant {
    pub(super) ty: Type,
    pub(super) bits: u64,
}

impl Expression<Constant> {
    /// Evaluate operations whose logical input normalization is already explicit.
    pub(super) fn carrier_result(&self, result: Type, component: usize) -> Option<u64> {
        let expression = match *self {
            // These operations explicitly interpret the source's logical width.
            Self::SignExtend { .. } | Self::BitCount { .. } => *self,
            _ => self.map(|input| Constant {
                ty: input.ty.carrier(),
                bits: input.ty.carrier().normalize(input.bits),
            }),
        };
        expression.constant_result(result.carrier(), component)
    }

    pub(super) fn constant_result(&self, result: Type, component: usize) -> Option<u64> {
        debug_assert!(component < self.result_types(result).len());
        let bits = match *self {
            Self::Binary {
                operator,
                left,
                right,
            } => integer::binary(left.ty, operator, left.bits, right.bits)?,
            Self::MultiplyWide {
                signed,
                left,
                right,
            } => {
                let product = if signed {
                    ((left.bits as i64 as i128) * (right.bits as i64 as i128)) as u128
                } else {
                    u128::from(left.bits) * u128::from(right.bits)
                };
                (product >> (component * 64)) as u64
            }
            Self::Compare {
                operator,
                left,
                right,
            } => u64::from(integer::compare(left.ty, operator, left.bits, right.bits)),
            Self::Shift {
                operator,
                value,
                count,
            } => integer::shift(value.ty, operator, value.bits, count.bits as u32),
            Self::Rotate {
                operator,
                value,
                count,
            } => integer::rotate(value.ty, operator, value.bits, count.bits as u32),
            Self::Select {
                condition,
                when_true,
                when_false,
            } => {
                if condition.bits != 0 {
                    when_true.bits
                } else {
                    when_false.bits
                }
            }
            Self::BitCount { operator, input } => {
                integer::bit_count(input.ty, operator, input.bits)
            }
            Self::SignExtend { input } => integer::signed_value(input.ty, input.bits) as u64,
            Self::ZeroTest { input, nonzero } => u64::from((input.bits != 0) == nonzero),
            Self::Convert { input } => input.bits,
            Self::LowBits { input, bits } => input.bits & integer::low_mask(bits),
        };
        Some(result.normalize(bits))
    }
}
