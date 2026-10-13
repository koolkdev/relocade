//! Partial remainders retain an exact value and the quotient's low bits.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32, I64};

use crate::floating::{RoundingInput, RoundingMode};

use super::{
    quotient, result::RoundedValue, rounding::FiniteMagnitude, value::KnownClass, ArithmeticResult,
    BinaryOperands,
};

pub(crate) struct Remainder {
    pub(crate) arithmetic: ArithmeticResult,
    /// The quotient magnitude supplies Q2/Q1/Q0, independently of operand signs.
    pub(crate) quotient: Val<I64>,
    pub(crate) incomplete: Val<I1>,
}

impl BinaryOperands {
    pub(crate) fn remainder(&self, body: &mut BlockBuilder<'_>) -> Result<Remainder, BuildError> {
        let (numerator, numerator_exponent) = self.left.normalized();
        let (denominator, denominator_exponent) = self.right.normalized();
        // Zero and a finite dividend modulo infinity are already reduced.
        let difference = self
            .left
            .zero
            .or(&self.right.infinity)
            .select(-2, numerator_exponent.sub(denominator_exponent));
        let incomplete = difference.signed().ge(64);
        // Intel permits an implementation-dependent reduction of 32..63 bits.
        // A 32-bit step bounds the work and always makes the required progress.
        let shift = incomplete.select(32, &difference);
        let (quotient, remainder, exponent) = body.if_value::<(I64, I64, I32)>(
            difference.signed().lt(1),
            |body| {
                let subtract = difference.eq(0).and(numerator.unsigned().ge(&denominator));
                body.yield_((
                    subtract.unsigned().extend::<I64>(),
                    numerator.sub(subtract.select(&denominator, 0_u64)),
                    &numerator_exponent,
                ))
            },
            |mut body| {
                let division = quotient::integer_division(
                    &mut body,
                    &numerator,
                    denominator.clone(),
                    &shift,
                    self.right.precision53_significand.is_some(),
                )?;
                body.yield_((
                    division.quotient,
                    division.remainder,
                    numerator_exponent.sub(&shift),
                ))
            },
        )?;

        let zero = remainder.eq(0_u64);
        let normalization = remainder.clz().truncate::<I32>();
        let magnitude = FiniteMagnitude {
            significand: RoundingInput::exact(remainder.shl(&normalization)),
            exponent: exponent.sub(normalization),
            negative: self.left.bits.negative(),
        };
        // Remainders are exact on the binary80 grid. Neither PC nor RC applies;
        // only an unmasked tiny-result exception needs the adjusted exponent.
        let mut arithmetic = magnitude.round(3.into(), &RoundingMode::new(0.into()));
        let signed_zero = RoundedValue::zero(self.left.bits.negative());
        arithmetic.replace_when(&zero, &signed_zero);
        arithmetic.in_range.valid = zero.or(&arithmetic.in_range.valid);
        arithmetic.in_range.rounded = signed_zero.select(&zero, &arithmetic.in_range.rounded);
        arithmetic.in_range.rounded.value = arithmetic
            .in_range
            .rounded
            .value
            .clone()
            .assume_class(KnownClass::zero().select(&zero, &KnownClass::normal()));
        if self.left.precision53_significand.is_some()
            && self.right.precision53_significand.is_some()
        {
            // A remainder cannot need more significant bits than both exact
            // inputs. Preserve this proof for later native arithmetic.
            arithmetic.in_range.rounded.value = arithmetic
                .in_range
                .rounded
                .value
                .clone()
                .assume_precision53();
        }
        let nan = self.left.nan.or(&self.right.nan);
        let invalid = nan.eq(false).and(self.left.infinity.or(&self.right.zero));
        let arithmetic = self.finish(arithmetic, invalid);
        let numerical = arithmetic.invalid.or(nan).eq(false);
        Ok(Remainder {
            arithmetic,
            // Condition codes are undefined for NaNs/invalid operations. Use
            // zero here while retaining a valid partial reduction's old Q bits.
            quotient: numerical.select(quotient, 0_u64),
            incomplete: numerical.and(incomplete),
        })
    }
}
