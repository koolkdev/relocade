//! Literal and body-owned handles, with visibility preserved across folding.

use std::marker::PhantomData;
use std::num::NonZeroUsize;

use super::Val;
use crate::{arena::ExpressionArena, BuildError, IntType, Type};

#[derive(Clone)]
pub(super) enum ValueSource {
    Literal(u64),
    Expression {
        arena: ExpressionArena,
        expression: Result<BoundExpression, BuildError>,
    },
}

/// Admission retains the original scope independently of the folded runtime node.
#[derive(Clone, Copy)]
pub(super) struct BoundExpression {
    pub(super) value: usize,
    required_scope: Option<NonZeroUsize>,
}

impl BoundExpression {
    pub(super) fn new(value: usize, required_scope: Option<usize>) -> Self {
        Self {
            value,
            // Scope indices come from a Vec, so its largest possible index is
            // below usize::MAX. Reserve zero for incompatible operand scopes.
            required_scope: required_scope.map(|scope| {
                NonZeroUsize::new(scope.checked_add(1).expect("scope index fits in usize"))
                    .expect("encoded scope is nonzero")
            }),
        }
    }

    pub(super) fn required_scope(self) -> Option<usize> {
        self.required_scope.map(|scope| scope.get() - 1)
    }

    pub(super) fn with_value(self, value: usize) -> Self {
        Self { value, ..self }
    }

    pub(super) fn admit(self, arena: &ExpressionArena, scope: usize) -> Result<usize, BuildError> {
        arena.require_visible(self.required_scope(), scope)?;
        Ok(self.value)
    }
}

impl ValueSource {
    pub(super) fn resolve(
        &self,
        arena: &ExpressionArena,
        ty: Type,
    ) -> Result<BoundExpression, BuildError> {
        match self {
            Self::Literal(bits) => Ok(BoundExpression::new(arena.constant(ty, *bits)?, Some(0))),
            Self::Expression {
                arena: owner,
                expression,
            } => {
                if !owner.same_body(arena) {
                    return Err(BuildError::ForeignBody);
                }
                arena.check_open()?;
                expression.clone()
            }
        }
    }
}

impl<T: IntType> Val<T> {
    // Authored parameters, reads, calls and joins begin with their runtime scope.
    // Calculations use `bound` to retain scopes that folding may discard.
    pub(crate) fn new(arena: ExpressionArena, expression: Result<usize, BuildError>) -> Self {
        let expression = expression
            .and_then(|value| Ok(BoundExpression::new(value, arena.required_scope(value)?)));
        Self::bound(arena, expression)
    }

    pub(super) fn bound(
        arena: ExpressionArena,
        expression: Result<BoundExpression, BuildError>,
    ) -> Self {
        Self {
            source: ValueSource::Expression { arena, expression },
            ty: PhantomData,
        }
    }

    pub(super) fn literal(bits: u64) -> Self {
        Self {
            source: ValueSource::Literal(T::TYPE.normalize(bits)),
            ty: PhantomData,
        }
    }

    pub(crate) fn checked_expression(
        &self,
        arena: &ExpressionArena,
        scope: usize,
    ) -> Result<usize, BuildError> {
        self.source.resolve(arena, T::TYPE)?.admit(arena, scope)
    }

    pub(crate) fn bind(&self, arena: &ExpressionArena, scope: usize) -> Result<Self, BuildError> {
        let expression = self.source.resolve(arena, T::TYPE)?;
        expression.admit(arena, scope)?;
        Ok(Self::bound(arena.clone(), Ok(expression)))
    }
}
