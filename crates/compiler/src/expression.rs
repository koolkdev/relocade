//! Pure expressions shared by unbound values and function bodies.

mod fold;
pub(crate) use fold::{build, map_inputs, normalize};

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
    /// Inputs in execution and Wasm stack order: select's condition comes last.
    pub(super) fn inputs(&self) -> impl DoubleEndedIterator<Item = &V> {
        let inputs = match self {
            Self::Binary { left, right, .. } | Self::Compare { left, right, .. } => {
                [Some(left), Some(right), None]
            }
            Self::Shift { value, count, .. } | Self::Rotate { value, count, .. } => {
                [Some(value), Some(count), None]
            }
            Self::Select {
                condition,
                when_true,
                when_false,
            } => [Some(when_true), Some(when_false), Some(condition)],
            Self::SignExtend { input }
            | Self::BitCount { input, .. }
            | Self::ZeroTest { input, .. }
            | Self::Convert { input }
            | Self::LowBits { input, .. } => [Some(input), None, None],
        };
        inputs.into_iter().flatten()
    }

    pub(super) fn map<U>(&self, mut input: impl FnMut(&V) -> U) -> Expression<U> {
        self.try_map(|value| Ok::<_, std::convert::Infallible>(input(value)))
            .unwrap_or_else(|error| match error {})
    }

    pub(super) fn try_map<U, E>(
        &self,
        mut input: impl FnMut(&V) -> Result<U, E>,
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
                condition: input(condition)?,
                when_true: input(when_true)?,
                when_false: input(when_false)?,
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
    pub(super) fn constant_result(&self, result: Type) -> Option<u64> {
        let bits = match *self {
            Self::Binary {
                operator,
                left,
                right,
            } => integer::binary(left.ty, operator, left.bits, right.bits)?,
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
