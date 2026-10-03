//! Numerical results carry values and exception evidence to the x87 state owner.

use wasm86_compiler::{BlockBuilder, BuildError, Results, Val, I1, I16, I64};

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

/// A complete binary response and its candidate for precision-only execution.
pub(crate) struct BinaryArithmetic {
    pub(crate) result: ArithmeticResult,
    pub(super) operands_valid: Val<I1>,
    /// Distinguishes a nonzero magnitude from exact zero after operand admission.
    /// The range check still belongs to the unrounded magnitude's candidate.
    pub(super) round_magnitude: Val<I1>,
    pub(super) zero: RoundedValue,
}

impl BinaryArithmetic {
    /// Joins magnitude and exact-zero paths before their shared precision response.
    /// Admission excludes operand exceptions; invalid ranges need the full response.
    pub(crate) fn rounding_candidate(
        &self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<ArithmeticCandidate, BuildError> {
        let (valid, components) = body.if_value::<(I1, RoundedShape)>(
            &self.round_magnitude,
            |body| {
                body.yield_((
                    &self.result.in_range.valid,
                    self.result.in_range.rounded.components(),
                ))
            },
            |body| body.yield_((true, self.zero.components())),
        )?;
        Ok(ArithmeticCandidate {
            valid: self.operands_valid.and(valid),
            rounded: RoundedValue::from_components(
                components,
                Classification::normal().select(&self.round_magnitude, &Classification::zero()),
            ),
        })
    }
}

impl RoundedValue {
    fn or_indefinite(&self, condition: &Val<I1>) -> Self {
        Self {
            value: self.value.or_indefinite(condition),
            inexact: condition.eq(false).and(&self.inexact),
            incremented: condition.eq(false).and(&self.incremented),
        }
    }

    pub(super) fn zero(negative: Val<I1>) -> Self {
        Self {
            value: ExtendedValue::from_bits(ExtendedBits {
                significand: 0_u64.into(),
                sign_exponent: negative.select(0x8000_u32, 0_u32),
            })
            .assume_class(Classification::zero()),
            inexact: false.into(),
            incremented: false.into(),
        }
    }

    pub(super) fn infinity(negative: Val<I1>) -> Self {
        Self {
            value: ExtendedValue::from_bits(ExtendedBits {
                significand: (1_u64 << 63).into(),
                sign_exponent: negative.select::<I16>(0xffff, 0x7fff),
            }),
            inexact: false.into(),
            incremented: false.into(),
        }
    }

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
    /// A precision-rounded candidate independent of the final masked/adjusted
    /// responses. The calculation owns operand admission; this range test alone
    /// does not exclude operand exceptions.
    pub(super) in_range: ArithmeticCandidate,
    pub(crate) invalid: Val<I1>,
    pub(crate) denormal: Val<I1>,
    pub(crate) zero_divide: Val<I1>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(super) masked: RoundedValue,
    pub(super) adjusted: RoundedValue,
}

impl ArithmeticResult {
    /// Replaces the final response with invalid-operation indefinite. State
    /// uses this for stack faults before recording any numerical exceptions;
    /// the independent rounding candidate is no longer consumed at that point.
    pub(crate) fn or_indefinite(mut self, condition: &Val<I1>) -> Self {
        self.masked = self.masked.or_indefinite(condition);
        self.adjusted = self.adjusted.or_indefinite(condition);
        self.overflow = condition.eq(false).and(&self.overflow);
        self.tiny = condition.eq(false).and(&self.tiny);
        self.invalid = condition.or(&self.invalid);
        self.denormal = condition.eq(false).and(&self.denormal);
        self.zero_divide = condition.eq(false).and(&self.zero_divide);
        self
    }

    /// Exact zero and special operands replace the final range responses.
    /// The magnitude candidate remains independent for specialization.
    pub(super) fn replace_when(&mut self, condition: &Val<I1>, rounded: &RoundedValue) {
        self.masked = rounded.select(condition, &self.masked);
        self.adjusted = rounded.select(condition, &self.adjusted);
        self.overflow = condition.eq(false).and(&self.overflow);
        self.tiny = condition.eq(false).and(&self.tiny);
    }

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
            zero_divide: false.into(),
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
