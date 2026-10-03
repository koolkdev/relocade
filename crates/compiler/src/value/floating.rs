//! Typed binary64 calculations and exact access to their encodings.

use crate::{
    floating::{BinaryOp, CompareOp, UnaryOp},
    Expression, Val, F64, I1, I64,
};

impl From<f64> for Val<F64> {
    fn from(value: f64) -> Self {
        Self::literal(value.to_bits())
    }
}

impl Val<F64> {
    /// Interprets an IEEE binary64 encoding without rounding or quieting NaNs.
    pub fn from_bits(bits: impl Into<Val<I64>>) -> Self {
        Self::expression(Expression::Reinterpret {
            input: bits.into().into(),
        })
    }

    /// Returns the exact IEEE binary64 encoding, including signed zero and NaN payloads.
    pub fn to_bits(&self) -> Val<I64> {
        Val::expression(Expression::Reinterpret { input: self.into() })
    }

    /// Adds with binary64 round-to-nearest, ties-to-even and gradual underflow.
    /// Arithmetic may choose any NaN permitted by scalar WebAssembly. Operations
    /// are not reassociated or contracted into a fused multiply-add.
    #[allow(clippy::should_implement_trait)]
    pub fn add(&self, other: impl Into<Self>) -> Self {
        self.binary(BinaryOp::Add, other)
    }

    /// Subtracts with the same strict binary64 behavior as [`Self::add`].
    #[allow(clippy::should_implement_trait)]
    pub fn sub(&self, other: impl Into<Self>) -> Self {
        self.binary(BinaryOp::Sub, other)
    }

    /// Multiplies with the same strict binary64 behavior as [`Self::add`].
    #[allow(clippy::should_implement_trait)]
    pub fn mul(&self, other: impl Into<Self>) -> Self {
        self.binary(BinaryOp::Mul, other)
    }

    /// Divides with strict binary64 behavior. Division by zero produces infinity
    /// or NaN as specified by IEEE arithmetic; it does not trap.
    #[allow(clippy::should_implement_trait)]
    pub fn div(&self, other: impl Into<Self>) -> Self {
        self.binary(BinaryOp::Div, other)
    }

    /// Clears the sign bit, preserving the payload even for a signaling NaN.
    pub fn abs(&self) -> Self {
        self.unary(UnaryOp::Abs)
    }

    /// Flips the sign bit, preserving the payload even for a signaling NaN.
    #[allow(clippy::should_implement_trait)]
    pub fn neg(&self) -> Self {
        self.unary(UnaryOp::Neg)
    }

    /// Numeric equality: signed zeros compare equal, and NaNs never compare equal.
    pub fn eq(&self, other: impl Into<Self>) -> Val<I1> {
        self.compare(CompareOp::Eq, other)
    }

    /// Numeric inequality, including true when either operand is a NaN.
    pub fn ne(&self, other: impl Into<Self>) -> Val<I1> {
        self.compare(CompareOp::Ne, other)
    }

    /// Less-than; false when either operand is a NaN.
    pub fn lt(&self, other: impl Into<Self>) -> Val<I1> {
        self.compare(CompareOp::Lt, other)
    }

    /// Less-than-or-equal; false when either operand is a NaN.
    pub fn le(&self, other: impl Into<Self>) -> Val<I1> {
        self.compare(CompareOp::Le, other)
    }

    /// Greater-than; false when either operand is a NaN.
    pub fn gt(&self, other: impl Into<Self>) -> Val<I1> {
        self.compare(CompareOp::Gt, other)
    }

    /// Greater-than-or-equal; false when either operand is a NaN.
    pub fn ge(&self, other: impl Into<Self>) -> Val<I1> {
        self.compare(CompareOp::Ge, other)
    }

    fn binary(&self, operator: BinaryOp, other: impl Into<Self>) -> Self {
        let other = other.into();
        Self::expression(Expression::FloatBinary {
            operator,
            left: self.into(),
            right: other.into(),
        })
    }

    fn unary(&self, operator: UnaryOp) -> Self {
        Self::expression(Expression::FloatUnary {
            operator,
            input: self.into(),
        })
    }

    fn compare(&self, operator: CompareOp, other: impl Into<Self>) -> Val<I1> {
        let other = other.into();
        Val::expression(Expression::FloatCompare {
            operator,
            left: self.into(),
            right: other.into(),
        })
    }
}
