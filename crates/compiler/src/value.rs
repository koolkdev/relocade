use std::marker::PhantomData;

use crate::arena::ExpressionArena;
use crate::{BuildError, IntType, I1, I64};

/// An integer value in one function body, with its type checked by Rust.
///
/// Operations build expressions immediately. Cloning a value shares the body's
/// storage; it does not clone the expression or its operands. A construction error
/// is reported when the resulting value is stored or returned. Returning
/// from or dropping the body prevents its values from building more expressions.
#[derive(Clone)]
pub struct Val<T: IntType> {
    arena: ExpressionArena,
    expression: Result<usize, BuildError>,
    ty: PhantomData<T>,
}

impl<T: IntType> Val<T> {
    pub(super) fn new(arena: ExpressionArena, expression: Result<usize, BuildError>) -> Self {
        Self {
            arena,
            expression,
            ty: PhantomData,
        }
    }

    pub(super) fn constant(arena: &ExpressionArena, value: impl IntLiteral<T>) -> Self {
        let bits = value.bits();
        Self::new(arena.clone(), arena.constant(T::TYPE, bits))
    }

    pub(super) fn admit(&self, arena: &ExpressionArena) -> Result<usize, BuildError> {
        if !self.arena.same_body(arena) {
            return Err(BuildError::ForeignBody);
        }
        arena.check_open()?;
        self.expression.clone()
    }

    /// Creates an independent constant in this value's body, with the requested type.
    pub fn c<To: IntType>(&self, value: impl IntLiteral<To>) -> Val<To> {
        Val::constant(&self.arena, value)
    }

    /// Adds an integer value or literal of the same type, wrapping on overflow.
    ///
    /// Different integer types cannot be mixed, even when both use Wasm i32:
    /// ```compile_fail
    /// use wasm86_compiler::{Val, I1, I8};
    /// fn add(left: &Val<I1>, right: &Val<I8>) {
    ///     let result = left.add(right);
    /// }
    /// ```
    /// A 64-bit literal requires a 64-bit receiver:
    /// ```compile_fail
    /// use wasm86_compiler::{Val, I32};
    /// fn add(value: &Val<I32>) {
    ///     let result = value.add(1_u64);
    /// }
    /// ```
    #[allow(clippy::should_implement_trait)]
    pub fn add(&self, other: impl IntoOp<T>) -> Self {
        // Conversion may construct a literal, so it must finish before borrowing
        // expression storage. Check both operands before a fold can discard either.
        let other = other.into_op(self);
        let expression = (|| {
            let left = self.admit(&self.arena)?;
            let right = other.admit(&self.arena)?;
            self.arena.add(left, right)
        })();
        Self::new(self.arena.clone(), expression)
    }
}

/// An integer literal accepted for a particular integer type.
///
/// `i32` and `u32` are reduced to the destination's low bits. For [`I64`],
/// `i32` is sign-extended and `u32` is zero-extended. A `u64` literal is accepted
/// only for [`I64`], and `bool` only for [`I1`].
/// ```compile_fail
/// use wasm86_compiler::{FunctionBuilder, I32};
/// fn constant(body: &FunctionBuilder<'_>) {
///     let value = body.constant::<I32>(1_u64);
/// }
/// ```
pub trait IntLiteral<T: IntType>: Copy {
    fn bits(self) -> u64;
}

impl<T: IntType> IntLiteral<T> for i32 {
    fn bits(self) -> u64 {
        self as i64 as u64
    }
}

impl<T: IntType> IntLiteral<T> for u32 {
    fn bits(self) -> u64 {
        u64::from(self)
    }
}

impl IntLiteral<I64> for u64 {
    fn bits(self) -> u64 {
        self
    }
}

impl IntLiteral<I1> for bool {
    fn bits(self) -> u64 {
        u64::from(self)
    }
}

/// A value of the receiver's integer type, or an accepted integer literal.
pub trait IntoOp<T: IntType> {
    /// Converts a literal in the receiver's body; values keep their own body.
    fn into_op(self, receiver: &Val<T>) -> Val<T>;
}

impl<T: IntType> IntoOp<T> for &Val<T> {
    fn into_op(self, _receiver: &Val<T>) -> Val<T> {
        self.clone()
    }
}

impl<T: IntType> IntoOp<T> for i32 {
    fn into_op(self, receiver: &Val<T>) -> Val<T> {
        receiver.c(self)
    }
}

impl<T: IntType> IntoOp<T> for u32 {
    fn into_op(self, receiver: &Val<T>) -> Val<T> {
        receiver.c(self)
    }
}

impl IntoOp<I64> for u64 {
    fn into_op(self, receiver: &Val<I64>) -> Val<I64> {
        receiver.c(self)
    }
}

impl IntoOp<I1> for bool {
    fn into_op(self, receiver: &Val<I1>) -> Val<I1> {
        receiver.c(self)
    }
}

#[cfg(test)]
mod tests;
