//! Literal, unbound and body-owned handles, with visibility preserved across folding.

use std::marker::PhantomData;
use std::num::NonZeroUsize;

use super::{UnboundExpression, Val};
use crate::{arena::FunctionArena, BuildError, IntType, Type};

#[derive(Clone)]
pub(crate) enum ValueSource {
    Literal(u64),
    Unbound(UnboundExpression),
    Expression {
        arena: FunctionArena,
        expression: Result<BoundExpression, BuildError>,
    },
}

/// Admission retains the original scope independently of the folded runtime node.
#[derive(Clone, Copy)]
pub(crate) struct BoundExpression {
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

    pub(super) fn admit(self, arena: &FunctionArena, scope: usize) -> Result<usize, BuildError> {
        arena.require_visible(self.required_scope(), scope)?;
        Ok(self.value)
    }
}

impl ValueSource {
    // Parameters, reads, calls and joins use the scope of their own definition.
    // Calculations retain their original operand scopes across folding.
    pub(crate) fn from_definition(
        arena: FunctionArena,
        expression: Result<usize, BuildError>,
    ) -> Self {
        let expression = expression.and_then(|value| {
            Ok(BoundExpression::new(
                value,
                Some(arena.definition_scope(value)?),
            ))
        });
        Self::Expression { arena, expression }
    }

    pub(super) fn resolve(
        &self,
        arena: &FunctionArena,
        ty: Type,
    ) -> Result<BoundExpression, BuildError> {
        match self {
            Self::Literal(bits) => Ok(BoundExpression::new(arena.constant(ty, *bits)?, Some(0))),
            Self::Unbound(expression) => Ok(BoundExpression::new(
                arena.resolve_unbound(expression)?,
                Some(0),
            )),
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

    pub(super) fn same_expression(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Literal(left), Self::Literal(right)) => left == right,
            (Self::Unbound(left), Self::Unbound(right)) => left == right,
            (
                Self::Expression {
                    arena: left_arena,
                    expression: left,
                },
                Self::Expression {
                    arena: right_arena,
                    expression: right,
                },
            ) => {
                left_arena.same_body(right_arena)
                    && matches!((left, right), (Ok(left), Ok(right)) if left.value == right.value)
            }
            _ => false,
        }
    }
}

impl<T: IntType> Val<T> {
    pub(crate) fn new(arena: FunctionArena, expression: Result<usize, BuildError>) -> Self {
        Self::from_source(ValueSource::from_definition(arena, expression))
    }

    pub(crate) fn from_source(source: ValueSource) -> Self {
        Self {
            source,
            ty: PhantomData,
        }
    }

    pub(super) fn bound(
        arena: FunctionArena,
        expression: Result<BoundExpression, BuildError>,
    ) -> Self {
        Self::from_source(ValueSource::Expression { arena, expression })
    }

    pub(super) fn literal(bits: u64) -> Self {
        Self::from_source(ValueSource::Literal(T::TYPE.normalize(bits)))
    }

    pub(crate) fn checked_expression(
        &self,
        arena: &FunctionArena,
        scope: usize,
    ) -> Result<usize, BuildError> {
        self.source.resolve(arena, T::TYPE)?.admit(arena, scope)
    }

    pub(crate) fn bind(&self, arena: &FunctionArena, scope: usize) -> Result<Self, BuildError> {
        let expression = self.source.resolve(arena, T::TYPE)?;
        expression.admit(arena, scope)?;
        Ok(Self::bound(arena.clone(), Ok(expression)))
    }
}
