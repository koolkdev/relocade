//! Partial remainders retain an exact value and the quotient's low bits.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32, I64};

use crate::floating::{quotient, FiniteMagnitude, RoundingInput, RoundingMode};

use super::{result::RoundedValue, value::KnownClass, ArithmeticResult, BinaryOperands};

pub(crate) struct Remainder {
    pub(crate) arithmetic: ArithmeticResult,
    /// The quotient magnitude supplies Q2/Q1/Q0, independently of operand signs.
    pub(crate) quotient: Val<I64>,
    pub(crate) incomplete: Val<I1>,
}

#[derive(Clone, Copy)]
pub(crate) enum QuotientRounding {
    Truncate,
    NearestEven,
}

impl BinaryOperands {
    pub(crate) fn remainder(
        &self,
        body: &mut BlockBuilder<'_>,
        rounding: QuotientRounding,
    ) -> Result<Remainder, BuildError> {
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
        let (mut quotient, mut remainder, exponent) = body.if_value::<(I64, I64, I32)>(
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

        let mut negative = self.left.bits.negative();
        if matches!(rounding, QuotientRounding::NearestEven) {
            // Compare the remainder with its complement to avoid overflowing
            // while doubling it. A tie rounds up only from an odd quotient.
            let complement = denominator.sub(&remainder);
            let above_half = complement.unsigned().lt(&remainder);
            let odd_tie = remainder.eq(&complement).and(quotient.and(1_u64).ne(0_u64));
            // With an exponent difference of -1 the quotient is zero, but its
            // nearest integer is one when the normalized numerator is larger.
            let half_exponent = difference.eq(-1);
            let increment = difference
                .signed()
                .ge(0)
                .and(above_half.or(odd_tie))
                .or(half_exponent.and(denominator.unsigned().lt(&numerator)))
                .and(incomplete.eq(false));
            let complement = half_exponent.select(denominator.shl(1).sub(&numerator), complement);
            remainder = increment.select(complement, remainder);
            quotient = quotient.add(increment.unsigned().extend::<I64>());
            negative = negative.xor(increment);
        }

        let zero = remainder.eq(0_u64);
        let normalization = remainder.clz().truncate::<I32>();
        let magnitude = FiniteMagnitude {
            significand: RoundingInput::exact(remainder.shl(&normalization)),
            exponent: exponent.sub(normalization),
            negative,
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
