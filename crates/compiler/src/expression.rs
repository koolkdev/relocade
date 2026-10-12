//! Pure expressions shared by unbound values and function bodies.

mod operators;
pub(crate) use operators::{BinaryOp, CompareOp, LaneBinaryOp, ShiftOp, UnaryOp};

mod evaluate;
pub(super) use evaluate::TypedLiteral;
mod fold;
mod lower;
mod shuffle;
pub(crate) use fold::{build, fold_carrier, normalize, refold};
pub(crate) use lower::lower;
pub(super) use shuffle::ShuffleMask;

#[cfg(test)]
mod tests;

use crate::Type;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum Expression<V> {
    Unary {
        operator: UnaryOp,
        input: V,
    },
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
    Select {
        condition: V,
        when_true: V,
        when_false: V,
    },
    MultiplyWide {
        signed: bool,
        left: V,
        right: V,
    },
}

impl<V> Expression<V> {
    /// Logical result types in their declared order. The caller supplies the
    /// value result type; operations with a fixed signature declare it here.
    pub(super) fn result_types(&self, ty: Type) -> impl ExactSizeIterator<Item = Type> {
        let (types, count) = match self {
            Self::MultiplyWide { .. } => ([Type::I64; 2], 2),
            _ => ([ty; 2], 1),
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
            Self::Unary {
                operator,
                input: value,
            } => Expression::Unary {
                operator: *operator,
                input: input(value)?,
            },
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
            Self::Select {
                condition,
                when_true,
                when_false,
            } => Expression::Select {
                when_true: input(when_true)?,
                when_false: input(when_false)?,
                condition: input(condition)?,
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
        })
    }
}
