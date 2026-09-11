//! Composition and admission of literal, unbound and body-owned calculations.
use super::{
    source::{BoundExpression, ValueSource},
    Val,
};
use crate::{arena::ExpressionArena, BuildError, IntType, I1};

impl<T: IntType> Val<T> {
    pub(super) fn map<R: IntType>(
        &self,
        literal: impl Fn(u64) -> u64,
        expression: impl Fn(&ExpressionArena, usize) -> Result<usize, BuildError> + 'static,
    ) -> Val<R> {
        match &self.source {
            ValueSource::Literal(bits) => Val::literal(literal(*bits)),
            ValueSource::Expression { arena, .. } => Val::bound(
                arena.clone(),
                self.source
                    .resolve(arena, T::TYPE)
                    .and_then(|input| Ok(input.with_value(expression(arena, input.value)?))),
            ),
            ValueSource::Unbound(_) => {
                let input = self.source.clone();
                Val::unbound(move |arena| expression(arena, input.resolve(arena, T::TYPE)?.value))
            }
        }
    }

    pub(super) fn combine<U: IntType, R: IntType>(
        &self,
        other: &Val<U>,
        literal: impl Fn(u64, u64) -> Option<u64>,
        expression: impl Fn(&ExpressionArena, usize, usize) -> Result<usize, BuildError> + 'static,
    ) -> Val<R> {
        if let (ValueSource::Literal(left), ValueSource::Literal(right)) =
            (&self.source, &other.source)
        {
            if let Some(bits) = literal(*left, *right) {
                return Val::literal(bits);
            }
        }
        match (&self.source, &other.source) {
            (ValueSource::Expression { arena, .. }, _)
            | (_, ValueSource::Expression { arena, .. }) => {
                // Resolve both inputs before a fold can discard either. A bound
                // result keeps its arena even when the resulting node is constant.
                let result = self.source.resolve(arena, T::TYPE).and_then(|left| {
                    let right = other.source.resolve(arena, U::TYPE)?;
                    let required_scope =
                        arena.merge_scopes(left.required_scope(), right.required_scope())?;
                    Ok(BoundExpression::new(
                        expression(arena, left.value, right.value)?,
                        required_scope,
                    ))
                });
                Val::bound(arena.clone(), result)
            }
            _ => {
                // A calculation with no constant result stays body-independent.
                // Later operations use this same composition path until admission.
                let left = self.source.clone();
                let right = other.source.clone();
                Val::unbound(move |arena| {
                    let left = left.resolve(arena, T::TYPE)?;
                    let right = right.resolve(arena, U::TYPE)?;
                    expression(arena, left.value, right.value)
                })
            }
        }
    }
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
    /// let function = program.declare(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I32],
    /// });
    /// let body = program.define(function)?;
    /// let value = body.parameter::<I32>(0)?;
    /// body.return_(value.eq(0).select(7, value.add(1)))?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn select<T: IntType>(
        &self,
        when_true: impl Into<Val<T>>,
        when_false: impl Into<Val<T>>,
    ) -> Val<T> {
        let when_true: Val<T> = when_true.into();
        let when_false: Val<T> = when_false.into();
        match (&self.source, &when_true.source, &when_false.source) {
            (
                ValueSource::Literal(condition),
                ValueSource::Literal(when_true),
                ValueSource::Literal(when_false),
            ) => Val::literal(if *condition != 0 {
                *when_true
            } else {
                *when_false
            }),
            (ValueSource::Expression { arena, .. }, _, _)
            | (_, ValueSource::Expression { arena, .. }, _)
            | (_, _, ValueSource::Expression { arena, .. }) => {
                // All three inputs participate in ownership and visibility, even
                // when a literal condition determines the selected alternative.
                let expression = self.source.resolve(arena, I1::TYPE).and_then(|condition| {
                    let when_true = when_true.source.resolve(arena, T::TYPE)?;
                    let when_false = when_false.source.resolve(arena, T::TYPE)?;
                    let alternatives = arena
                        .merge_scopes(when_true.required_scope(), when_false.required_scope())?;
                    let required_scope =
                        arena.merge_scopes(condition.required_scope(), alternatives)?;
                    Ok(BoundExpression::new(
                        arena.select(condition.value, when_true.value, when_false.value)?,
                        required_scope,
                    ))
                });
                Val::bound(arena.clone(), expression)
            }
            _ => {
                let condition = self.source.clone();
                let when_true = when_true.source;
                let when_false = when_false.source;
                Val::unbound(move |arena| {
                    let condition = condition.resolve(arena, I1::TYPE)?;
                    let when_true = when_true.resolve(arena, T::TYPE)?;
                    let when_false = when_false.resolve(arena, T::TYPE)?;
                    arena.select(condition.value, when_true.value, when_false.value)
                })
            }
        }
    }
}
