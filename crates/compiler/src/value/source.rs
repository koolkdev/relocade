//! Literal, unbound and body-owned handles, with visibility preserved across folding.

use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::NonZeroUsize;
use std::rc::Rc;

use super::Val;
use crate::{arena::ExpressionArena, BuildError, IntType, Type};

#[derive(Clone)]
pub(super) enum ValueSource {
    Literal(u64),
    Unbound(UnboundExpression),
    Expression {
        arena: ExpressionArena,
        expression: Result<BoundExpression, BuildError>,
    },
}

// Recipes capture only unbound operands and operators, never an arena. An open
// arena can therefore retain resolved recipe identities without an Rc cycle.
type ExpressionRecipe = dyn Fn(&ExpressionArena) -> Result<usize, BuildError>;

#[derive(Clone)]
pub(crate) struct UnboundExpression(Rc<ExpressionRecipe>);

impl UnboundExpression {
    pub(crate) fn build(&self, arena: &ExpressionArena) -> Result<usize, BuildError> {
        self.0(arena)
    }
}

impl PartialEq for UnboundExpression {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for UnboundExpression {}

impl Hash for UnboundExpression {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Rc::as_ptr(&self.0) as *const ()).hash(state);
    }
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

    pub(super) fn unbound(
        expression: impl Fn(&ExpressionArena) -> Result<usize, BuildError> + 'static,
    ) -> Self {
        Self {
            source: ValueSource::Unbound(UnboundExpression(Rc::new(expression))),
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
