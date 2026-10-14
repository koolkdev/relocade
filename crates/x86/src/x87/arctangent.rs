//! Quadrants and exponent differences stay exact before a bounded atan kernel.

use wasm86_compiler::{BlockBuilder, BuildError, I1, I32, I64};

use crate::floating::{RoundingInput, RoundingMode};

use super::{
    quotient,
    result::RoundedValue,
    rounding::FiniteMagnitude,
    transcendental::{odd_power_series, DoubleDouble},
    ArithmeticResult, BinaryOperands,
};

impl BinaryOperands {
    pub(crate) fn arctangent(
        &self,
        body: &mut BlockBuilder<'_>,
        rounding: &RoundingMode,
    ) -> Result<ArithmeticResult, BuildError> {
        let y = &self.left;
        let x = &self.right;
        let (y_significand, y_exponent) = y.normalized();
        let (x_significand, x_exponent) = x.normalized();
        let y_significand = y.zero.select(1_u64 << 63, y_significand);
        let x_significand = x.zero.select(1_u64 << 63, x_significand);
        let swap = x_exponent.signed().lt(&y_exponent).or(x_exponent
            .eq(&y_exponent)
            .and(x_significand.unsigned().lt(&y_significand)));
        let numerator = swap.select(&x_significand, &y_significand);
        let denominator = swap.select(&y_significand, &x_significand);
        let gap = swap.select(x_exponent.sub(&y_exponent), y_exponent.sub(&x_exponent));
        let pi = DoubleDouble::pi();
        let half_pi = pi.scale(&(-1).into());
        let (integer, guard, sticky, exponent) = body.if_value::<(I64, I1, I1, I32)>(
            gap.signed().lt(-120),
            |body| {
                // A rational quotient can lie closer to a rounding boundary than
                // DD preserves. Keep its exact remainder before subtracting the
                // much smaller atan tail; at this gap that tail cannot cross a
                // nonzero remainder boundary of two 64-bit significands.
                let quotient = quotient::calculate(&numerator, denominator.clone(), false);
                let ratio = FiniteMagnitude {
                    significand: quotient.significand,
                    exponent: gap.sub(quotient.below_one.unsigned().extend::<I32>()),
                    negative: y.bits.negative(),
                }
                .with_negative_tail(&true.into());
                let offset = swap.or(x.bits.negative());
                let axis =
                    half_pi
                        .select(&swap, &pi)
                        .magnitude(0.into(), y.bits.negative(), true.into());
                body.yield_((
                    offset.select(axis.significand.integer, ratio.significand.integer),
                    offset.select(axis.significand.guard, ratio.significand.guard),
                    offset.select(axis.significand.sticky, ratio.significand.sticky),
                    offset.select(axis.exponent, ratio.exponent),
                ))
            },
            |mut body| {
                let ratio = DoubleDouble::significand(&numerator)
                    .divide(&DoubleDouble::significand(&denominator));
                let angle = atan(&mut body, &ratio.scale(&gap))?;
                let angle = half_pi.sub(&angle).select(&swap, &angle);
                let angle = pi.sub(&angle).select(&x.bits.negative(), &angle);
                let magnitude = angle.magnitude(0.into(), y.bits.negative(), true.into());
                body.yield_((
                    magnitude.significand.integer,
                    magnitude.significand.guard,
                    magnitude.significand.sticky,
                    magnitude.exponent,
                ))
            },
        )?;
        let mut result = FiniteMagnitude {
            significand: RoundingInput {
                integer,
                guard,
                sticky,
            },
            exponent,
            negative: y.bits.negative(),
        }
        .round(3.into(), rounding);
        for rounded in [
            &mut result.in_range.rounded,
            &mut result.masked,
            &mut result.adjusted,
        ] {
            rounded.inexact = true.into();
        }

        // atan2 defines 0/0 and infinity/infinity through their quadrants, with
        // no invalid or zero-divide exception. Preserve the original y sign.
        let both_infinite = x.infinity.and(&y.infinity);
        let on_x_axis = y.zero.or(&x.infinity);
        let zero = on_x_axis
            .and(both_infinite.eq(false))
            .and(x.bits.negative().eq(false));
        let diagonal = pi.scale(&(-2).into());
        let diagonal = pi.sub(&diagonal).select(&x.bits.negative(), &diagonal);
        let axis = pi.select(&on_x_axis, &half_pi);
        let special = diagonal
            .select(&both_infinite, &axis)
            .magnitude(0.into(), y.bits.negative(), true.into())
            .round(3.into(), rounding);
        let rounded =
            RoundedValue::zero(y.bits.negative()).select(&zero, &special.in_range.rounded);
        let special_operand = x.zero.or(&y.zero).or(&x.infinity).or(&y.infinity);
        result.replace_when(&special_operand, &rounded);
        result.in_range.rounded = rounded.select(&special_operand, &result.in_range.rounded);
        result.in_range.valid = special_operand.or(result.in_range.valid);
        Ok(self.finish(result, false.into()))
    }
}

/// Reduce [0,1] to [-1/3,1/2], then atan(z) = z * odd_power_series(-z²).
/// Forty-eight terms give a relative tail below 2^-102. DD roundoff and the
/// quadrant constants keep the angle error below 2^-88 before binary80 rounding.
fn atan(body: &mut BlockBuilder<'_>, argument: &DoubleDouble) -> Result<DoubleDouble, BuildError> {
    let offset = argument.parts().0.ge(0.5);
    let one = DoubleDouble::from_f64(1.0);
    let reduced = argument
        .sub(&one)
        .divide(&argument.add(&one))
        .select(&offset, argument);
    let series = odd_power_series(body, &reduced.mul(&reduced).neg(), 48)?;
    let angle = series.mul(&reduced);
    Ok(angle
        .add(&DoubleDouble::pi().scale(&(-2).into()))
        .select(&offset, &angle))
}
