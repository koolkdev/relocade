//! Composition and admission of literal, unbound and body-owned expressions.
use super::{
    source::{BoundExpression, ValueSource},
    unbound::{Operand as UnboundOperand, UnboundExpression},
    Val,
};
use crate::{expression::Constant, results, Expression, Results, Type, ValueType, I1};

pub(super) struct Operand {
    ty: Type,
    source: ValueSource,
}

impl<T: ValueType> From<&Val<T>> for Operand {
    fn from(value: &Val<T>) -> Self {
        Self {
            ty: T::TYPE,
            source: value.source.clone(),
        }
    }
}

impl<T: ValueType> From<Val<T>> for Operand {
    fn from(value: Val<T>) -> Self {
        Self {
            ty: T::TYPE,
            source: value.source,
        }
    }
}

impl<T: ValueType> Val<T> {
    pub(super) fn expression(expression: Expression<Operand>) -> Self {
        expression_results::<T>(expression)
    }
}

/// Bind a semantic operation's outputs to the same typed shapes as calls and joins.
pub(super) fn expression_results<R: Results>(expression: Expression<Operand>) -> R::Values {
    let arena = expression
        .inputs()
        .find_map(|operand| match &operand.source {
            ValueSource::Expression { arena, .. } => Some(arena.clone()),
            _ => None,
        });
    if let Some(arena) = arena {
        // Every original input contributes ownership and visibility before
        // folding can discard it, including either arm of a constant select.
        let mut required_scope = Some(0);
        let bound = expression.try_map(|operand| {
            let input = operand.source.resolve(&arena, operand.ty)?;
            required_scope = arena.merge_scopes(required_scope, input.required_scope())?;
            Ok(input.value)
        });
        let mut component = 0;
        results::bind_sources::<R>(|ty| {
            let expression = bound.clone().and_then(|expression| {
                Ok(BoundExpression::new(
                    arena.expression(ty, expression, component)?,
                    required_scope,
                ))
            });
            component += 1;
            ValueSource::Expression {
                arena: arena.clone(),
                expression,
            }
        })
    } else {
        unbound_results::<R>(expression)
    }
}

fn unbound_results<R: Results>(expression: Expression<Operand>) -> R::Values {
    let constants = expression
        .try_map(|operand| match operand.source {
            ValueSource::Literal(bits) => Ok(Constant {
                ty: operand.ty,
                bits,
            }),
            _ => Err(()),
        })
        .ok();
    let mut recipe: Option<UnboundExpression> = None;
    let mut component = 0;
    results::bind_sources::<R>(|ty| {
        let result = if let Some(bits) =
            constants.and_then(|constants| constants.constant_result(ty, component))
        {
            ValueSource::Literal(bits)
        } else {
            let recipe = recipe.get_or_insert_with(|| {
                let expression = expression.map(|operand| match &operand.source {
                    ValueSource::Literal(bits) => UnboundOperand::Literal(Constant {
                        ty: operand.ty,
                        bits: *bits,
                    }),
                    ValueSource::Unbound(expression) => {
                        UnboundOperand::Expression(expression.clone())
                    }
                    ValueSource::Expression { .. } => {
                        unreachable!("a body operand supplies its arena")
                    }
                });
                UnboundExpression::new(ty, expression)
            });
            ValueSource::Unbound(recipe.result(component))
        };
        component += 1;
        result
    })
}

impl Val<I1> {
    /// Chooses one of two values. Both alternatives are eager inputs; this
    /// operation does not guard either alternative. Use `if_value` for branch-local
    /// work. Constant choices may discard unused expressions after validation.
    /// The alternatives have the same logical type. Two literal alternatives need
    /// an explicit type, for example `condition.select::<I32>(7, 9)`.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I32],
    /// }, |body| {
    ///     let value = body.parameter::<I32>(0)?;
    ///     body.return_(value.eq(0).select(7, value.add(1)))
    /// })?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn select<T: ValueType>(
        &self,
        when_true: impl Into<Val<T>>,
        when_false: impl Into<Val<T>>,
    ) -> Val<T> {
        let when_true: Val<T> = when_true.into();
        let when_false: Val<T> = when_false.into();
        Val::expression(Expression::Select {
            condition: self.into(),
            when_true: when_true.into(),
            when_false: when_false.into(),
        })
    }
}
