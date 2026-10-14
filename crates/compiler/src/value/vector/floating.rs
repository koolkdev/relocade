//! Borrowed floating interpretations keep vector storage and integer lanes distinct.

use crate::{
    expression::{BinaryOp as Binary, UnaryOp as Unary},
    floating::{BinaryOp, CompareOp, Format, UnaryOp, VectorConvertOp},
    Val, V128,
};

impl Val<V128> {
    /// Interpret these bits as four IEEE binary32 lanes. Arithmetic uses strict
    /// Wasm SIMD: nearest, ties-to-even, gradual underflow and Wasm NaN choices.
    /// Operations return raw vector encodings and do not report exception flags.
    ///
    /// ```
    /// use wasm86_compiler::{Val, V128};
    /// let left = Val::<V128>::from([0_u8; 16]);
    /// let right = Val::<V128>::from([0_u8; 16]);
    /// let sum = left.f32x4().add(&right);
    /// let integral = sum.f32x4().nearest();
    /// ```
    pub fn f32x4(&self) -> F32x4<'_> {
        F32x4(self)
    }

    /// Interpret these bits as two IEEE binary64 lanes, with the same strict
    /// floating-point contract as [`Self::f32x4`].
    pub fn f64x2(&self) -> F64x2<'_> {
        F64x2(self)
    }

    /// Convert four signed I32 lanes to binary32 with nearest, ties-to-even rounding.
    pub fn signed_i32_lanes_to_f32(&self) -> Self {
        self.unary(Unary::FloatVectorConvert(VectorConvertOp::FromSignedI32 {
            format: Format::Binary32,
        }))
    }

    /// Convert the low two signed I32 lanes exactly to binary64. The upper two
    /// source lanes do not participate in the conversion.
    pub fn low_signed_i32_lanes_to_f64(&self) -> Self {
        self.unary(Unary::FloatVectorConvert(VectorConvertOp::FromSignedI32 {
            format: Format::Binary64,
        }))
    }
}

macro_rules! floating_view {
    ($name:ident, $format:ident, $description:literal) => {
        #[doc = $description]
        pub struct $name<'a>(&'a Val<V128>);

        impl $name<'_> {
            /// Add corresponding lanes with nearest, ties-to-even rounding.
            /// Arithmetic is not reassociated or contracted into fused operations.
            #[allow(clippy::should_implement_trait)]
            pub fn add(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.binary(BinaryOp::Add, right.into())
            }

            /// Subtract corresponding lanes with strict Wasm floating semantics.
            #[allow(clippy::should_implement_trait)]
            pub fn sub(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.binary(BinaryOp::Sub, right.into())
            }

            /// Multiply corresponding lanes with strict Wasm floating semantics.
            #[allow(clippy::should_implement_trait)]
            pub fn mul(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.binary(BinaryOp::Mul, right.into())
            }

            /// Divide corresponding lanes. Division by zero does not trap.
            #[allow(clippy::should_implement_trait)]
            pub fn div(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.binary(BinaryOp::Div, right.into())
            }

            /// Clear each sign bit, preserving NaN payloads, including signaling NaNs.
            pub fn abs(&self) -> Val<V128> {
                self.unary(UnaryOp::Abs)
            }

            /// Flip each sign bit, preserving all remaining encoding bits.
            #[allow(clippy::should_implement_trait)]
            pub fn neg(&self) -> Val<V128> {
                self.unary(UnaryOp::Neg)
            }

            /// Round each lane toward positive infinity, preserving signed zero.
            pub fn ceil(&self) -> Val<V128> {
                self.unary(UnaryOp::Ceil)
            }

            /// Round each lane toward negative infinity, preserving signed zero.
            pub fn floor(&self) -> Val<V128> {
                self.unary(UnaryOp::Floor)
            }

            /// Round each lane toward zero, preserving its sign.
            pub fn trunc(&self) -> Val<V128> {
                self.unary(UnaryOp::Trunc)
            }

            /// Round each lane to the nearest integer, ties to even.
            pub fn nearest(&self) -> Val<V128> {
                self.unary(UnaryOp::Nearest)
            }

            /// Compute each square root with nearest, ties-to-even rounding.
            /// Negative nonzero lanes produce a Wasm arithmetic NaN.
            pub fn sqrt(&self) -> Val<V128> {
                self.unary(UnaryOp::Sqrt)
            }

            /// All-one masks for numerically equal lanes; signed zeros are equal.
            /// A lane containing a NaN never compares equal.
            pub fn eq(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.compare(CompareOp::Eq, right.into())
            }

            /// All-one masks for unequal lanes, including unordered NaN pairs.
            pub fn ne(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.compare(CompareOp::Ne, right.into())
            }

            /// All-one masks for ordered less-than comparisons.
            pub fn lt(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.compare(CompareOp::Lt, right.into())
            }

            /// All-one masks for ordered less-than-or-equal comparisons.
            pub fn le(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.compare(CompareOp::Le, right.into())
            }

            /// All-one masks for ordered greater-than comparisons.
            pub fn gt(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.compare(CompareOp::Gt, right.into())
            }

            /// All-one masks for ordered greater-than-or-equal comparisons.
            pub fn ge(&self, right: impl Into<Val<V128>>) -> Val<V128> {
                self.compare(CompareOp::Ge, right.into())
            }

            fn unary(&self, operator: UnaryOp) -> Val<V128> {
                self.0.unary(Unary::FloatVector {
                    operator,
                    format: Format::$format,
                })
            }

            fn binary(&self, operator: BinaryOp, right: Val<V128>) -> Val<V128> {
                self.0.binary(
                    Binary::FloatVector {
                        operator,
                        format: Format::$format,
                    },
                    right,
                )
            }

            fn compare(&self, operator: CompareOp, right: Val<V128>) -> Val<V128> {
                self.0.binary(
                    Binary::FloatVectorCompare {
                        operator,
                        format: Format::$format,
                    },
                    right,
                )
            }
        }
    };
}

floating_view!(
    F32x4,
    Binary32,
    "A borrowed interpretation of V128 as four binary32 lanes."
);
floating_view!(
    F64x2,
    Binary64,
    "A borrowed interpretation of V128 as two binary64 lanes."
);

impl F32x4<'_> {
    /// Promote the low two lanes to binary64. Finite encodings are exact; NaNs
    /// follow Wasm promotion semantics. The upper source lanes are ignored.
    pub fn promote_low(&self) -> Val<V128> {
        self.0.unary(Unary::FloatVectorConvert(
            VectorConvertOp::PromoteLowBinary32,
        ))
    }

    /// Truncate toward zero to four signed I32 lanes. Out-of-range lanes saturate
    /// to the appropriate bound; NaNs produce zero. This operation never traps.
    pub fn trunc_saturating_i32(&self) -> Val<V128> {
        self.0.unary(Unary::FloatVectorConvert(
            VectorConvertOp::TruncateSaturatingI32 {
                format: Format::Binary32,
            },
        ))
    }
}

impl F64x2<'_> {
    /// Round to binary32 with nearest, ties-to-even and gradual underflow, then
    /// clear the upper two lanes. NaNs follow Wasm demotion semantics.
    pub fn demote_zero(&self) -> Val<V128> {
        self.0.unary(Unary::FloatVectorConvert(
            VectorConvertOp::DemoteBinary64Zero,
        ))
    }

    /// Truncate toward zero to two signed I32 lanes and clear the upper two lanes.
    /// Out-of-range values saturate; NaNs produce zero. This never traps.
    pub fn trunc_saturating_i32_zero(&self) -> Val<V128> {
        self.0.unary(Unary::FloatVectorConvert(
            VectorConvertOp::TruncateSaturatingI32 {
                format: Format::Binary64,
            },
        ))
    }
}
