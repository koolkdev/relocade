//! Signed integer values and static bounds carried across exact x87 arithmetic.

use relocade_compiler::{Val, I1, I64};

use super::{ExtendedBits, ExtendedValue, KnownClass, Precision53};
use crate::{
    floating::RoundingMode,
    x87::{result::RoundedValue, ArithmeticCandidate, BinaryOperands, BinaryOperation},
};

/// The coefficient equals the numerical value whenever its encoding is normal
/// or zero. A masked stack fault can replace that encoding with indefinite.
#[derive(Clone)]
pub(in crate::x87) struct IntegerOrigin {
    pub(in crate::x87) coefficient: Val<I64>,
    minimum: i64,
    maximum: i64,
}

impl IntegerOrigin {
    pub(in crate::x87) fn signed_input(coefficient: Val<I64>, bits: u32) -> Self {
        let shift = 64 - bits;
        Self {
            coefficient,
            minimum: i64::MIN >> shift,
            maximum: i64::MAX >> shift,
        }
    }

    pub(in crate::x87) fn fits_signed(&self, bits: u32) -> bool {
        self.minimum >= (i64::MIN >> (64 - bits)) && self.maximum <= (i64::MAX >> (64 - bits))
    }

    pub(super) fn select(&self, condition: &Val<I1>, other: &Self) -> Self {
        Self {
            coefficient: condition.select(&self.coefficient, &other.coefficient),
            minimum: self.minimum.min(other.minimum),
            maximum: self.maximum.max(other.maximum),
        }
    }

    fn arithmetic(&self, right: &Self, operation: BinaryOperation, precision: u32) -> Option<Self> {
        // Widen only the static bounds. Even the product of two signed 64-bit
        // endpoints fits in i128; no wide operation is emitted to Wasm.
        let (a, b) = (i128::from(self.minimum), i128::from(self.maximum));
        let (c, d) = (i128::from(right.minimum), i128::from(right.maximum));
        let (minimum, maximum, coefficient) = match operation {
            BinaryOperation::Add => (a + c, b + d, self.coefficient.add(&right.coefficient)),
            BinaryOperation::Subtract => (a - d, b - c, self.coefficient.sub(&right.coefficient)),
            BinaryOperation::ReverseSubtract => {
                (c - b, d - a, right.coefficient.sub(&self.coefficient))
            }
            BinaryOperation::Multiply => {
                let corners = [a * c, a * d, b * c, b * d];
                (
                    corners.into_iter().min()?,
                    corners.into_iter().max()?,
                    self.coefficient.mul(&right.coefficient),
                )
            }
            BinaryOperation::Divide | BinaryOperation::ReverseDivide => return None,
        };
        let limit = 1_i128 << precision;
        if minimum < -limit || maximum > limit {
            return None;
        }
        Some(Self {
            coefficient,
            minimum: i64::try_from(minimum).ok()?,
            maximum: i64::try_from(maximum).ok()?,
        })
    }

    fn into_value(self, zero_negative: Val<I1>) -> ExtendedValue {
        let zero = self.coefficient.eq(0_u64);
        let negative = self.coefficient.signed().lt(0);
        let magnitude = negative.select(
            Val::<I64>::from(0_u64).sub(&self.coefficient),
            &self.coefficient,
        );
        let bits = ExtendedBits::from_integer_magnitude(
            &magnitude,
            &zero.select(&zero_negative, &negative),
        );
        let mut value = if self.minimum >= -(1_i64 << 53) && self.maximum <= 1_i64 << 53 {
            // Integer conversion loses the sign of zero. Keep that sign in the
            // native view too so a subsequent floating-point operation sees it.
            let native = zero
                .and(zero_negative)
                .select(-0.0, self.coefficient.signed().to_f64());
            ExtendedValue::from_precision53(Precision53::from_binary64(&bits, native))
        } else {
            ExtendedValue::from_bits(bits)
        };
        value.integer_origin = Some(self);
        value.assume_class(KnownClass::zero().select(&zero, &KnownClass::normal()))
    }
}

impl BinaryOperands {
    pub(in crate::x87) fn exact_integer_candidate(
        &self,
        operation: BinaryOperation,
        rounding: &RoundingMode,
        precision_control: Option<u64>,
    ) -> Option<ArithmeticCandidate> {
        let precision = match precision_control? {
            0 => 24,
            2 => 53,
            3 => 64,
            _ => return None,
        };
        let result = self.left.integer_origin.as_ref()?.arithmetic(
            self.right.integer_origin.as_ref()?,
            operation,
            precision,
        )?;
        let signs = self.addend_signs(operation);
        let zero_negative = if matches!(operation, BinaryOperation::Multiply) {
            signs.left_negative.xor(&signs.right_negative)
        } else {
            signs
                .left_negative
                .eq(&signs.right_negative)
                .select(&signs.left_negative, rounding.cancellation_negative())
        };
        Some(ArithmeticCandidate {
            valid: self.normal_or_zero(),
            rounded: RoundedValue::exact(result.into_value(zero_negative)),
        })
    }
}
