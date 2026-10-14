//! Tangent divides unrounded components and keeps exact evidence near a pole.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I32};

use crate::floating::{RoundingInput, RoundingMode};

use super::super::{
    quotient,
    result::RoundedValue,
    rounding::{FiniteMagnitude, MagnitudeShape},
    transcendental::DoubleDouble,
    ArithmeticResult, ExtendedBits, ExtendedValue,
};
use super::{series, TrigonometricArgument};

impl TrigonometricArgument<'_> {
    pub(crate) fn tangent(
        &self,
        body: &mut BlockBuilder<'_>,
        rounding: &RoundingMode,
    ) -> Result<[ArithmeticResult; 2], BuildError> {
        let odd = self.reduced.quadrant.and(1).ne(0);
        let negative = odd
            .xor(&self.reduced.negative)
            .xor(self.operand.bits.negative());
        let small_original = self.exponent.signed().lt(-32).or(&self.operand.zero);
        let small_remainder = self
            .reduced
            .magnitude
            .parts()
            .0
            .lt(odd.select(2_f64.powi(-48), 2_f64.powi(-32)));
        let magnitude = body.if_value::<MagnitudeShape>(
            small_original.or(small_remainder),
            |mut body| {
                let reduced =
                    self.reduced
                        .magnitude
                        .magnitude(0.into(), negative.clone(), false.into());
                let magnitude = body.if_value::<MagnitudeShape>(
                    &odd,
                    |body| {
                        // Here r = K*2^-65 with K < 2^17. The exact reciprocal's
                        // nonzero distance to an integer/half boundary exceeds
                        // 2^-18 ulp; 1/r-cot(r) is less than 2^64*r²/2 < 2^-33
                        // ulp. The negative tail cannot cross such a boundary.
                        let quotient = quotient::calculate(
                            &(1_u64 << 63).into(),
                            reduced.significand.integer.clone(),
                            true,
                        );
                        let magnitude = FiniteMagnitude {
                            significand: quotient.significand,
                            exponent: Val::<I32>::from(0)
                                .sub(&reduced.exponent)
                                .sub(quotient.below_one.unsigned().extend::<I32>()),
                            negative: negative.clone(),
                        }
                        .with_negative_tail(&true.into());
                        body.yield_(magnitude.components())
                    },
                    |body| {
                        // tan(r)>r by less than half an ulp below 2^-32. Both
                        // the original operand and this small reduced remainder
                        // have exact dyadic significands, including subnormals.
                        let magnitude = FiniteMagnitude {
                            significand: RoundingInput {
                                integer: small_original
                                    .select(&self.significand, &reduced.significand.integer),
                                guard: false.into(),
                                sticky: self.operand.zero.eq(false),
                            },
                            exponent: small_original.select(&self.exponent, &reduced.exponent),
                            negative: negative.clone(),
                        };
                        body.yield_(magnitude.components())
                    },
                )?;
                body.yield_(magnitude)
            },
            |mut body| {
                let sine = series(&mut body, &self.reduced.magnitude, &false.into())?;
                let difference = series(&mut body, &self.reduced.magnitude, &true.into())?;
                let cosine = DoubleDouble::from_f64(1.0).sub(&difference);
                let numerator = cosine.select(&odd, &sine);
                let denominator = sine.select(&odd, &cosine);
                body.yield_(
                    numerator
                        .divide(&denominator)
                        .magnitude(0.into(), negative.clone(), true.into())
                        .components(),
                )
            },
        )?;
        let tangent = self.finish(
            FiniteMagnitude::from_components(magnitude),
            rounding,
            RoundedValue::zero(self.operand.bits.negative()),
        );
        // A NaN or masked invalid operand produces two exceptional outputs,
        // not a numeric one above a NaN tangent.
        let one = ArithmeticResult::from_rounding(RoundedValue::exact(ExtendedValue::from_bits(
            ExtendedBits {
                significand: (1_u64 << 63).into(),
                sign_exponent: 0x3fff_u32.into(),
            },
        )));
        Ok([
            tangent,
            self.operand.finish(one, self.operand.infinity.clone()),
        ])
    }
}
