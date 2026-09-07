use std::marker::PhantomData;

use crate::arena::ExpressionArena;
use crate::{
    integer::{BinaryOp, CompareOp, ShiftOp},
    AtLeast, BuildError, IntType, Type, I1, I64,
};

/// An integer value in one function body, with its type checked by Rust.
///
/// Operations build expressions immediately. Cloning a value shares the body's
/// storage; it does not clone the expression or its operands. A construction error
/// is reported when the resulting value is stored, returned or passed to a call.
/// Completing or dropping the body prevents its values from building more expressions.
#[derive(Clone)]
pub struct Val<T: IntType> {
    arena: ExpressionArena,
    expression: Result<usize, BuildError>,
    ty: PhantomData<T>,
}

/// A call argument retaining its logical type and function-body ownership.
/// Create one with [`Val::argument`] to pass differently typed values together.
#[derive(Clone)]
pub struct Argument {
    arena: ExpressionArena,
    expression: Result<usize, BuildError>,
    ty: Type,
}

impl Argument {
    pub(super) fn admit(
        &self,
        arena: &ExpressionArena,
        expected: Type,
    ) -> Result<usize, BuildError> {
        let value = admit(&self.arena, arena, &self.expression)?;
        if self.ty != expected {
            return Err(BuildError::TypeMismatch {
                expected,
                actual: self.ty,
            });
        }
        Ok(value)
    }
}

fn admit(
    owner: &ExpressionArena,
    body: &ExpressionArena,
    expression: &Result<usize, BuildError>,
) -> Result<usize, BuildError> {
    if !owner.same_body(body) {
        return Err(BuildError::ForeignBody);
    }
    body.check_open()?;
    expression.clone()
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
        admit(&self.arena, arena, &self.expression)
    }

    /// Passes this value in a call argument list while retaining its logical type and body.
    pub fn argument(&self) -> Argument {
        Argument {
            arena: self.arena.clone(),
            expression: self.expression.clone(),
            ty: T::TYPE,
        }
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
        self.binary(BinaryOp::Add, other)
    }

    /// Keeps bits set in both operands.
    pub fn and(&self, other: impl IntoOp<T>) -> Self {
        self.binary(BinaryOp::And, other)
    }

    /// Sets bits present in either operand.
    pub fn or(&self, other: impl IntoOp<T>) -> Self {
        self.binary(BinaryOp::Or, other)
    }

    /// Shifts left, retaining the logical type's low bits.
    /// Counts are modulo 32 for I1/I8/I16/I32, and modulo 64 for I64.
    /// In particular, an I8 shift by 8 produces zero; a shift by 32 is identity.
    #[allow(clippy::should_implement_trait)]
    pub fn shl(&self, count: u32) -> Self {
        self.shift(ShiftOp::Left, count)
    }

    /// Tests whether the operands' logical low bits are equal.
    pub fn eq(&self, other: impl IntoOp<T>) -> Val<I1> {
        self.compare(CompareOp::Eq, other)
    }

    /// Tests whether the operands' logical low bits differ.
    pub fn ne(&self, other: impl IntoOp<T>) -> Val<I1> {
        self.compare(CompareOp::Ne, other)
    }

    /// Reads this integer's logical bits as an unsigned value.
    /// Creating the view does not construct an expression.
    pub fn unsigned(&self) -> Unsigned<'_, T> {
        Unsigned(self)
    }

    /// Retains the destination type's low bits. The destination cannot be wider.
    /// ```compile_fail
    /// use wasm86_compiler::{Val, I8, I32};
    /// fn narrow(value: &Val<I8>) {
    ///     let result = value.truncate::<I32>();
    /// }
    /// ```
    pub fn truncate<To: IntType>(&self) -> Val<To>
    where
        T: AtLeast<To>,
    {
        self.convert()
    }

    fn operands(&self, other: impl IntoOp<T>) -> Result<(usize, usize), BuildError> {
        // Literal conversion may construct expressions. Finish it before borrowing
        // storage, and check both operands before a fold can discard either.
        let other = other.into_op(self);
        Ok((self.admit(&self.arena)?, other.admit(&self.arena)?))
    }

    fn binary(&self, operator: BinaryOp, other: impl IntoOp<T>) -> Self {
        let expression = self
            .operands(other)
            .and_then(|(left, right)| self.arena.binary(operator, left, right));
        Self::new(self.arena.clone(), expression)
    }

    fn compare(&self, operator: CompareOp, other: impl IntoOp<T>) -> Val<I1> {
        let expression = self
            .operands(other)
            .and_then(|(left, right)| self.arena.compare(operator, left, right));
        Val::new(self.arena.clone(), expression)
    }

    fn shift(&self, operator: ShiftOp, count: u32) -> Self {
        let expression = self
            .admit(&self.arena)
            .and_then(|input| self.arena.shift(operator, input, count));
        Self::new(self.arena.clone(), expression)
    }

    fn convert<To: IntType>(&self) -> Val<To> {
        let expression = self
            .admit(&self.arena)
            .and_then(|input| self.arena.convert(input, To::TYPE));
        Val::new(self.arena.clone(), expression)
    }
}

/// An unsigned interpretation of a borrowed integer value.
pub struct Unsigned<'a, T: IntType>(&'a Val<T>);

impl<T: IntType> Unsigned<'_, T> {
    /// Shifts the logical bits right, filling with zeros.
    /// Counts are modulo 32 for I1/I8/I16/I32, and modulo 64 for I64.
    /// An I8 shift by 8 produces zero, including for values with bit 7 set.
    #[allow(clippy::should_implement_trait)]
    pub fn shr(&self, count: u32) -> Val<T> {
        self.0.shift(ShiftOp::Right, count)
    }

    /// Tests unsigned less-than and returns a logical one-bit value.
    pub fn lt(&self, other: impl IntoOp<T>) -> Val<I1> {
        self.0.compare(CompareOp::Lt, other)
    }

    /// Tests unsigned greater-than-or-equal and returns a logical one-bit value.
    pub fn ge(&self, other: impl IntoOp<T>) -> Val<I1> {
        self.0.compare(CompareOp::Ge, other)
    }

    /// Widens the logical value with zero bits. The destination cannot be narrower.
    /// ```compile_fail
    /// use wasm86_compiler::{Val, I8, I32};
    /// fn widen(value: &Val<I32>) {
    ///     let result = value.unsigned().extend::<I8>();
    /// }
    /// ```
    pub fn extend<To: AtLeast<T>>(&self) -> Val<To> {
        self.0.convert()
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
