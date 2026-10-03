//! Numerical results carry values and exception evidence to the x87 state owner.

use wasm86_compiler::{Results, Val, I1, I16, I64};

use super::{value::Classification, ExtendedBits, ExtendedValue};

/// An extended result and the rounding evidence needed for #P and C1.
#[derive(Clone)]
pub(crate) struct RoundedValue {
    pub(crate) value: ExtendedValue,
    /// Rounding lost information from the exact mathematical result.
    pub(crate) inexact: Val<I1>,
    /// Rounding increased the magnitude above its truncated value.
    pub(crate) incremented: Val<I1>,
}

/// Logical components at a compiler value join: bits and rounding evidence.
pub(super) type RoundedShape = (I64, I16, I1, I1);

/// The rounded bits are usable when `valid` is true. Operand exceptions remain
/// the operation's responsibility; a range candidate alone does not exclude them.
pub(crate) struct ArithmeticCandidate {
    pub(crate) valid: Val<I1>,
    pub(crate) rounded: RoundedValue,
}

impl RoundedValue {
    pub(super) fn components(&self) -> <RoundedShape as Results>::Values {
        let bits = self.value.bits();
        (
            bits.significand,
            bits.sign_exponent,
            self.inexact.clone(),
            self.incremented.clone(),
        )
    }

    /// The caller must establish the class wherever these joined bits are used.
    pub(super) fn from_components(
        components: <RoundedShape as Results>::Values,
        class: Classification,
    ) -> Self {
        let (significand, sign_exponent, inexact, incremented) = components;
        Self {
            value: ExtendedValue::from_bits(ExtendedBits {
                significand,
                sign_exponent,
            })
            .assume_class(class),
            inexact,
            incremented,
        }
    }

    pub(super) fn select(&self, condition: &Val<I1>, otherwise: &Self) -> Self {
        Self {
            value: self.value.select(condition, &otherwise.value),
            inexact: condition.select(&self.inexact, &otherwise.inexact),
            incremented: condition.select(&self.incremented, &otherwise.incremented),
        }
    }
}

/// A register result whose range response is selected after applying exception masks.
pub(crate) struct ArithmeticResult {
    /// The precision-rounded result before any range response.
    /// Precision loss and its rounding-direction evidence remain observable.
    pub(super) in_range: ArithmeticCandidate,
    pub(crate) invalid: Val<I1>,
    pub(crate) denormal: Val<I1>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(super) masked: RoundedValue,
    pub(super) adjusted: RoundedValue,
}

impl ArithmeticResult {
    /// Builds a response whose only possible arithmetic exception is inexact (#P).
    /// The caller must exclude operand and range exceptions first. The rounding
    /// evidence still controls C1 and the masked or unmasked #P response.
    pub(crate) fn from_rounding(rounded: RoundedValue) -> Self {
        Self {
            in_range: ArithmeticCandidate {
                valid: true.into(),
                rounded: rounded.clone(),
            },
            invalid: false.into(),
            denormal: false.into(),
            overflow: false.into(),
            tiny: false.into(),
            masked: rounded.clone(),
            adjusted: rounded,
        }
    }

    /// Unmasked register range exceptions commit the precision-rounded value
    /// with an adjusted exponent, retaining that value's precision evidence.
    pub(crate) fn resolve_range(&self, unmasked: &Val<I1>) -> RoundedValue {
        self.adjusted.select(unmasked, &self.masked)
    }
}

/// Result bits and numerical conditions from conversion to the destination format.
/// The x87 state applies exception masks to these conditions.
///
/// Invalid conversions do not report inexactness or a rounding increment.
pub(crate) struct ConversionResult {
    pub(crate) bits: Val<I64>,
    pub(crate) invalid: Val<I1>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(crate) inexact: Val<I1>,
    /// Rounding increased the magnitude.
    pub(crate) incremented: Val<I1>,
}
