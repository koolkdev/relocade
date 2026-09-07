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
/// is reported when the resulting value is stored, yielded, returned or passed
/// to a call.
/// Completing or dropping the outer function builder closes expression construction.
/// Values depending on a child read, call or join can only be consumed in that
/// child or its descendants.
#[derive(Clone)]
pub struct Val<T: IntType> {
    arena: ExpressionArena,
    expression: Result<usize, BuildError>,
    ty: PhantomData<T>,
}

/// An integer value or literal supplied where a function signature determines its type.
/// Values keep their logical type and body; literals use the expected type.
/// Signed i32 literals sign-extend to I64; u32 literals zero-extend. Both reduce
/// to the low bits for narrower types. A u64 literal requires I64, and bool requires I1.
#[derive(Clone)]
pub struct Argument(Operand);

#[derive(Clone)]
enum Operand {
    Value {
        arena: ExpressionArena,
        expression: Result<usize, BuildError>,
        ty: Type,
    },
    Signed(i32),
    Unsigned(u32),
    Wide(u64),
    Bit(bool),
}

impl Argument {
    pub(super) fn admit(
        &self,
        arena: &ExpressionArena,
        expected: Type,
    ) -> Result<usize, BuildError> {
        let bits = match &self.0 {
            Operand::Value {
                arena: owner,
                expression,
                ty,
            } => {
                let value = admit(owner, arena, expression)?;
                if *ty != expected {
                    return Err(BuildError::TypeMismatch {
                        expected,
                        actual: *ty,
                    });
                }
                return Ok(value);
            }
            Operand::Signed(value) => *value as i64 as u64,
            Operand::Unsigned(value) => u64::from(*value),
            Operand::Wide(value) if expected == Type::I64 => *value,
            Operand::Bit(value) if expected == Type::I1 => u64::from(*value),
            Operand::Wide(_) => {
                return Err(BuildError::TypeMismatch {
                    expected,
                    actual: Type::I64,
                })
            }
            Operand::Bit(_) => {
                return Err(BuildError::TypeMismatch {
                    expected,
                    actual: Type::I1,
                })
            }
        };
        arena.constant(expected, bits)
    }
}

impl<T: IntType> From<&Val<T>> for Argument {
    fn from(value: &Val<T>) -> Self {
        Self(Operand::Value {
            arena: value.arena.clone(),
            expression: value.expression.clone(),
            ty: T::TYPE,
        })
    }
}

impl<T: IntType> From<Val<T>> for Argument {
    fn from(value: Val<T>) -> Self {
        Self(Operand::Value {
            arena: value.arena,
            expression: value.expression,
            ty: T::TYPE,
        })
    }
}

impl From<i32> for Argument {
    fn from(value: i32) -> Self {
        Self(Operand::Signed(value))
    }
}
impl From<u32> for Argument {
    fn from(value: u32) -> Self {
        Self(Operand::Unsigned(value))
    }
}
impl From<u64> for Argument {
    fn from(value: u64) -> Self {
        Self(Operand::Wide(value))
    }
}
impl From<bool> for Argument {
    fn from(value: bool) -> Self {
        Self(Operand::Bit(value))
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

    pub(super) fn admit(&self, arena: &ExpressionArena) -> Result<usize, BuildError> {
        admit(&self.arena, arena, &self.expression)
    }

    /// Passes this value in a call argument list while retaining its logical type and body.
    pub fn argument(&self) -> Argument {
        self.into()
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
        // Check both operands before a fold can discard either.
        let other = other.into();
        Ok((self.admit(&self.arena)?, other.admit(&self.arena, T::TYPE)?))
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

/// A typed integer value or an integer literal accepted for that type.
/// Literal operands are constructed in the body consuming the operand.
pub trait IntoOp<T: IntType>: Into<Argument> {}

impl<T: IntType> IntoOp<T> for &Val<T> {}
impl<T: IntType> IntoOp<T> for Val<T> {}
impl<T: IntType> IntoOp<T> for i32 {}
impl<T: IntType> IntoOp<T> for u32 {}
impl IntoOp<I64> for u64 {}
impl IntoOp<I1> for bool {}

#[cfg(test)]
mod tests;
