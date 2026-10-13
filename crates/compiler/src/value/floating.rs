//! Typed binary64 calculations and exact access to their encodings.

use super::{Signed, Unsigned};
use crate::expression::{BinaryOp as Binary, CompareOp as Compare, UnaryOp as Unary};
use crate::{
    floating::{BinaryOp, CompareOp, ConvertOp, UnaryOp},
    IntType, Val, F64, I1, I32, I64,
};

impl From<f64> for Val<F64> {
    fn from(value: f64) -> Self {
        Self::literal(value.to_bits())
    }
}

impl Val<F64> {
    /// Promotes an IEEE binary32 encoding to binary64. Finite values, infinities
    /// and signed zeros are exact; NaNs follow WebAssembly promotion semantics.
    /// The source bits remain available when their exact payload must be retained.
    pub fn from_binary32_bits(bits: impl Into<Val<I32>>) -> Self {
        bits.into()
            .unary(Unary::FloatConvert(ConvertOp::PromoteBinary32))
    }

    /// Interprets an IEEE binary64 encoding without rounding or quieting NaNs.
    pub fn from_bits(bits: impl Into<Val<I64>>) -> Self {
        bits.into().unary(Unary::Reinterpret)
    }

    /// Returns the exact IEEE binary64 encoding, including signed zero and NaN payloads.
    pub fn to_bits(&self) -> Val<I64> {
        self.unary(Unary::Reinterpret)
    }

    /// Truncates toward zero to a signed 32-bit integer. Out-of-range values
    /// saturate to the nearest bound and NaNs produce zero; this never traps.
    pub fn trunc_saturating_i32(&self) -> Val<I32> {
        self.unary(Unary::FloatConvert(ConvertOp::TruncateSaturatingI32))
    }

    /// Adds with binary64 round-to-nearest, ties-to-even and gradual underflow.
    /// Arithmetic may choose any NaN permitted by scalar WebAssembly. Operations
    /// are not reassociated or contracted into a fused multiply-add.
    #[allow(clippy::should_implement_trait)]
    pub fn add(&self, other: impl Into<Self>) -> Self {
        self.binary(Binary::Float(BinaryOp::Add), other)
    }

    /// Subtracts with the same strict binary64 behavior as [`Self::add`].
    #[allow(clippy::should_implement_trait)]
    pub fn sub(&self, other: impl Into<Self>) -> Self {
        self.binary(Binary::Float(BinaryOp::Sub), other)
    }

    /// Multiplies with the same strict binary64 behavior as [`Self::add`].
    #[allow(clippy::should_implement_trait)]
    pub fn mul(&self, other: impl Into<Self>) -> Self {
        self.binary(Binary::Float(BinaryOp::Mul), other)
    }

    /// Divides with strict binary64 behavior. Division by zero produces infinity
    /// or NaN as specified by IEEE arithmetic; it does not trap.
    #[allow(clippy::should_implement_trait)]
    pub fn div(&self, other: impl Into<Self>) -> Self {
        self.binary(Binary::Float(BinaryOp::Div), other)
    }

    /// Clears the sign bit, preserving the payload even for a signaling NaN.
    pub fn abs(&self) -> Self {
        self.unary(Unary::Float(UnaryOp::Abs))
    }

    /// Flips the sign bit, preserving the payload even for a signaling NaN.
    #[allow(clippy::should_implement_trait)]
    pub fn neg(&self) -> Self {
        self.unary(Unary::Float(UnaryOp::Neg))
    }

    /// Rounds toward positive infinity, preserving signed zero. NaNs follow
    /// scalar WebAssembly arithmetic semantics, including quieting signaling NaNs.
    pub fn ceil(&self) -> Self {
        self.unary(Unary::Float(UnaryOp::Ceil))
    }

    /// Rounds toward negative infinity with the same NaN behavior as [`Self::ceil`].
    pub fn floor(&self) -> Self {
        self.unary(Unary::Float(UnaryOp::Floor))
    }

    /// Rounds toward zero, preserving its sign, with the NaN behavior of [`Self::ceil`].
    pub fn trunc(&self) -> Self {
        self.unary(Unary::Float(UnaryOp::Trunc))
    }

    /// Rounds to the nearest integer, ties to even, preserving signed zero.
    /// NaNs follow the same arithmetic semantics as [`Self::ceil`].
    pub fn nearest(&self) -> Self {
        self.unary(Unary::Float(UnaryOp::Nearest))
    }

    /// Computes a binary64 square root, rounding to nearest, ties to even.
    /// Signed zeros are preserved; negative nonzero operands produce an
    /// arithmetic NaN as specified by scalar WebAssembly.
    pub fn sqrt(&self) -> Self {
        self.unary(Unary::Float(UnaryOp::Sqrt))
    }

    /// Numeric equality: signed zeros compare equal, and NaNs never compare equal.
    pub fn eq(&self, other: impl Into<Self>) -> Val<I1> {
        self.binary(Binary::Compare(Compare::Float(CompareOp::Eq)), other)
    }

    /// Numeric inequality, including true when either operand is a NaN.
    pub fn ne(&self, other: impl Into<Self>) -> Val<I1> {
        self.binary(Binary::Compare(Compare::Float(CompareOp::Ne)), other)
    }

    /// Less-than; false when either operand is a NaN.
    pub fn lt(&self, other: impl Into<Self>) -> Val<I1> {
        self.binary(Binary::Compare(Compare::Float(CompareOp::Lt)), other)
    }

    /// Less-than-or-equal; false when either operand is a NaN.
    pub fn le(&self, other: impl Into<Self>) -> Val<I1> {
        self.binary(Binary::Compare(Compare::Float(CompareOp::Le)), other)
    }

    /// Greater-than; false when either operand is a NaN.
    pub fn gt(&self, other: impl Into<Self>) -> Val<I1> {
        self.binary(Binary::Compare(Compare::Float(CompareOp::Gt)), other)
    }

    /// Greater-than-or-equal; false when either operand is a NaN.
    pub fn ge(&self, other: impl Into<Self>) -> Val<I1> {
        self.binary(Binary::Compare(Compare::Float(CompareOp::Ge)), other)
    }
}

impl<T: IntType> Signed<'_, T> {
    /// Converts the signed logical integer to binary64, rounding to nearest,
    /// ties to even. Inputs of up to 32 bits are exact; I1's true value is -1.
    pub fn to_f64(&self) -> Val<F64> {
        self.0
            .unary(Unary::FloatConvert(ConvertOp::FromSignedInteger))
    }
}

impl<T: IntType> Unsigned<'_, T> {
    /// Converts the unsigned logical integer to binary64, rounding to nearest,
    /// ties to even. Inputs of up to 32 bits are exact.
    pub fn to_f64(&self) -> Val<F64> {
        self.0
            .unary(Unary::FloatConvert(ConvertOp::FromUnsignedInteger))
    }
}
