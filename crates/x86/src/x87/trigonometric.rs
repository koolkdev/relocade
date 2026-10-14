//! Trigonometric functions share exact quadrant reduction and bounded series.

mod reduction;
mod tangent;

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I32, I64};

use crate::floating::{RoundingInput, RoundingMode};

use super::{
    result::RoundedValue,
    rounding::{FiniteMagnitude, MagnitudeShape},
    transcendental::{DoubleDouble, DoubleShape},
    ArithmeticResult, ExtendedBits, ExtendedValue, Operand,
};

#[derive(Clone, Copy)]
pub(crate) enum TrigonometricFunction {
    Sine,
    Cosine,
}

/// One reduction can serve either component or both components of a pair.
/// The original significand/exponent remain exact for arbitrarily small angles.
pub(crate) struct TrigonometricArgument<'a> {
    operand: &'a Operand,
    significand: Val<I64>,
    exponent: Val<I32>,
    reduced: reduction::ReducedAngle,
}

impl Operand {
    pub(crate) fn trigonometric_domain(&self) -> Val<I1> {
        self.bits.exponent_field().unsigned().lt(16383 + 63)
    }

    pub(crate) fn trigonometric_out_of_range(&self) -> Val<I1> {
        self.normal_or_zero()
            .and(self.trigonometric_domain().eq(false))
    }
}

impl<'a> TrigonometricArgument<'a> {
    pub(crate) fn new(
        body: &mut BlockBuilder<'_>,
        operand: &'a Operand,
    ) -> Result<Self, BuildError> {
        let (significand, exponent) = operand.normalized();
        let significand = operand.zero.select(1_u64 << 63, significand);
        let (parts, negative, quadrant) = body.if_value::<(DoubleShape, I1, I32)>(
            exponent.signed().ge(-1).and(exponent.signed().lt(63)),
            |mut body| {
                let reduced = reduction::reduce(&mut body, &significand, &exponent)?;
                body.yield_((
                    reduced.magnitude.parts(),
                    reduced.negative,
                    reduced.quadrant,
                ))
            },
            |body| {
                // Tiny angles use the original integer magnitude later. Special
                // and out-of-range operands replace this harmless calculation.
                let bounded = exponent.signed().ge(-120).and(exponent.signed().lt(-1));
                let angle =
                    DoubleDouble::significand(&significand).scale(&bounded.select(&exponent, -2));
                body.yield_((angle.parts(), false, 0))
            },
        )?;
        Ok(Self {
            operand,
            significand,
            exponent,
            reduced: reduction::ReducedAngle {
                magnitude: DoubleDouble::from_parts(parts),
                negative,
                quadrant,
            },
        })
    }

    pub(crate) fn evaluate(
        &self,
        body: &mut BlockBuilder<'_>,
        function: TrigonometricFunction,
        rounding: &RoundingMode,
    ) -> Result<ArithmeticResult, BuildError> {
        let cosine = matches!(function, TrigonometricFunction::Cosine);
        let odd_quadrant = self.reduced.quadrant.and(1).ne(0);
        let kernel_cosine = odd_quadrant.xor(cosine);
        let negative = if cosine {
            self.reduced
                .quadrant
                .add(1)
                .and(2)
                .ne(0)
                .xor(odd_quadrant.and(&self.reduced.negative))
        } else {
            self.reduced
                .quadrant
                .and(2)
                .ne(0)
                .xor(odd_quadrant.eq(false).and(&self.reduced.negative))
                .xor(self.operand.bits.negative())
        };
        let small_original = self.exponent.signed().lt(-32).or(&self.operand.zero);
        let small_remainder = self.reduced.magnitude.parts().0.lt(2_f64.powi(-32));
        let magnitude = body.if_value::<MagnitudeShape>(
            small_original.or(small_remainder),
            |body| {
                // Below 2^-32, sine differs from the exact angle by less than
                // half a binary80 ulp, and cosine differs from one by less than
                // half the ulp below one. A small reduced remainder has at most
                // 33 bits (integer * 2^-65), so it too has exact integer evidence.
                let reduced =
                    self.reduced
                        .magnitude
                        .magnitude(0.into(), negative.clone(), false.into());
                let magnitude = FiniteMagnitude {
                    significand: RoundingInput {
                        integer: kernel_cosine.select(
                            1_u64 << 63,
                            small_original.select(&self.significand, reduced.significand.integer),
                        ),
                        guard: false.into(),
                        sticky: false.into(),
                    },
                    exponent: kernel_cosine
                        .select(0, small_original.select(&self.exponent, reduced.exponent)),
                    negative: negative.clone(),
                }
                .with_negative_tail(&self.operand.zero.eq(false));
                body.yield_(magnitude.components())
            },
            |mut body| {
                let value = series(&mut body, &self.reduced.magnitude, &kernel_cosine)?;
                let magnitude = body.if_value::<MagnitudeShape>(
                    &kernel_cosine,
                    |body| {
                        body.yield_(cosine_from_difference(&value, negative.clone()).components())
                    },
                    |body| {
                        body.yield_(
                            value
                                .magnitude(0.into(), negative.clone(), true.into())
                                .components(),
                        )
                    },
                )?;
                body.yield_(magnitude)
            },
        )?;
        let at_zero = if cosine {
            RoundedValue::exact(ExtendedValue::from_bits(ExtendedBits {
                significand: (1_u64 << 63).into(),
                sign_exponent: 0x3fff_u32.into(),
            }))
        } else {
            RoundedValue::zero(self.operand.bits.negative())
        };
        Ok(self.finish(
            FiniteMagnitude::from_components(magnitude),
            rounding,
            at_zero,
        ))
    }

    fn finish(
        &self,
        magnitude: FiniteMagnitude,
        rounding: &RoundingMode,
        at_zero: RoundedValue,
    ) -> ArithmeticResult {
        let mut result = magnitude.round(3.into(), rounding);
        let outside = self.operand.trigonometric_out_of_range();
        let unchanged = RoundedValue::exact(ExtendedValue::from_bits(self.operand.bits.clone()));
        let exact = unchanged.select(&outside, &at_zero);
        let exact_case = outside.or(&self.operand.zero);
        result.replace_when(&exact_case, &exact);
        result.in_range.rounded = exact.select(&exact_case, &result.in_range.rounded);
        result.in_range.valid = exact_case.or(result.in_range.valid);
        self.operand.finish(result, self.operand.infinity.clone())
    }
}

/// The 18-term sine and (1-cosine) recurrences on [0,pi/4] have relative truncation
/// error below 2^-140. DD roundoff keeps the combined error below 2^-88.
/// Choosing the initial term and factorial parity computes only the component
/// required by the reduced quadrant, without evaluating a second series.
fn series(
    body: &mut BlockBuilder<'_>,
    argument: &DoubleDouble,
    cosine: &Val<I1>,
) -> Result<DoubleDouble, BuildError> {
    let square = argument.mul(argument);
    let initial = square.scale(&(-1).into()).select(cosine, argument);
    let square = square.neg();
    let first = cosine.select::<I32>(3, 2);
    let parts = body.loop_::<(I32, DoubleShape, DoubleShape), DoubleShape>(
        (&first, initial.parts(), initial.parts()),
        |mut iteration, labels, (index, term, sum)| {
            iteration.if_(index.eq(first.add(34)), |done| {
                done.branch(&labels.exit, sum.clone())
            })?;
            let denominator = index.mul(index.add(1)).unsigned().to_f64();
            let term = DoubleDouble::from_parts(term)
                .mul(&square)
                .divide(&DoubleDouble::from_f64(denominator));
            let sum = DoubleDouble::from_parts(sum).add(&term);
            iteration.branch(&labels.again, (index.add(2), term.parts(), sum.parts()))
        },
    )?;
    Ok(DoubleDouble::from_parts(parts))
}

/// Keep 1-cosine separate until the binary80 grid has been chosen. Adding a
/// correction near 2^-65 to one in DD would discard bits that decide whether
/// cosine lies above or below the midpoint between one and its predecessor.
fn cosine_from_difference(difference: &DoubleDouble, negative: Val<I1>) -> FiniteMagnitude {
    let difference = difference.magnitude(0.into(), false.into(), true.into());
    // cos(t) is in [sqrt(1/2), 1), so use its fixed lower binade. The integer
    // part of one on this grid is 2^64; wrapping subtraction encodes that exactly.
    let difference = difference
        .significand
        .shift_right(Val::<I32>::from(-1).sub(difference.exponent));
    let fractional = difference.guard.or(&difference.sticky);
    FiniteMagnitude {
        significand: RoundingInput {
            integer: Val::<I64>::from(0_u64)
                .sub(difference.integer)
                .sub(fractional.unsigned().extend::<I64>()),
            guard: difference.guard.xor(&difference.sticky),
            sticky: difference.sticky,
        },
        exponent: (-1).into(),
        negative,
    }
}
