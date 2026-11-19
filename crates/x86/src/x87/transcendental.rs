//! Extra significand precision for bounded transcendental calculations.
//!
//! Each value is the unevaluated sum of two binary64 components. Strict Wasm
//! arithmetic retains the addition/product residuals; the guest exponent stays
//! separate so the binary64 exponent range does not limit binary80 operands.

use relocade_compiler::{BlockBuilder, BuildError, Val, F64, I1, I32, I64};

use crate::floating::{FiniteMagnitude, RoundingInput};

#[cfg(test)]
mod tests;

pub(super) type DoubleShape = (F64, F64);

#[derive(Clone)]
pub(super) struct DoubleDouble {
    high: Val<F64>,
    low: Val<F64>,
}

impl DoubleDouble {
    pub(super) fn from_parts((high, low): (Val<F64>, Val<F64>)) -> Self {
        Self { high, low }
    }

    pub(super) fn parts(&self) -> (Val<F64>, Val<F64>) {
        (self.high.clone(), self.low.clone())
    }

    pub(super) fn from_f64(value: impl Into<Val<F64>>) -> Self {
        Self {
            high: value.into(),
            low: 0.0.into(),
        }
    }

    pub(super) fn ln_2() -> Self {
        Self {
            high: f64::from_bits(0x3fe6_2e42_fefa_39ef).into(),
            low: f64::from_bits(0x3c7a_bc9e_3b39_803f).into(),
        }
    }

    pub(super) fn pi() -> Self {
        Self {
            high: f64::from_bits(0x4009_21fb_5444_2d18).into(),
            low: f64::from_bits(0x3ca1_a626_3314_5c07).into(),
        }
    }

    /// Converts all 64 significand bits exactly, with bit 63 representing one.
    pub(super) fn significand(bits: &Val<I64>) -> Self {
        Self::sum(
            bits.unsigned()
                .shr(32)
                .unsigned()
                .to_f64()
                .mul(2_f64.powi(-31)),
            bits.and(0xffff_ffff_u64)
                .unsigned()
                .to_f64()
                .mul(2_f64.powi(-63)),
        )
    }

    fn sum(left: Val<F64>, right: Val<F64>) -> Self {
        let high = left.add(&right);
        let right_part = high.sub(&left);
        let low = left.sub(high.sub(&right_part)).add(right.sub(right_part));
        Self { high, low }
    }

    pub(super) fn add(&self, other: &Self) -> Self {
        let sum = Self::sum(self.high.clone(), other.high.clone());
        Self::sum(sum.high, sum.low.add(self.low.add(&other.low)))
    }

    /// Requires finite normalized pairs and a nonzero sum. Each series forms
    /// its next term by multiplying by the fixed factor and may divide the
    /// term or its contribution by an integer between 2 and 2048.
    pub(super) fn absorbs_series_tail(&self, term: &Self, factor: &Self) -> Val<I1> {
        // The nearer rounding midpoint of either nonzero component is at least
        // 2^-54 times its magnitude, even at a binade boundary. A term below
        // half that bound for the low component leaves both words unchanged:
        // TwoSum(H, t.high) gives (H, t.high), and the two additions to L
        // both round to L. Renormalizing (H, L) then returns the original pair.
        let bound = self.low.abs().mul(2_f64.powi(-55));
        // For B >= 2^-500 and |term.high| <= B, splitting introduces less
        // than 2^-24 B error in either part relative to (term.high, 0).
        // Bounding the split products and low cross terms in mul gives a
        // high-word error below 2^-18 B for |factor.high| <= 3/4. In divide
        // the reconstructed estimate times the divisor is within 2^-16 B of
        // the numerator, so the corrected quotient is below 1.02 B/divisor.
        // Both updates stay within B. The lower bound on B keeps even errors
        // from gradual underflow far below this margin in later iterations.
        let absorbed = bound.ge(2_f64.powi(-500)).and(term.high.abs().lt(bound));
        // A normalized zero term remains zero. Adding it could still turn
        // a negative-zero low component into positive zero.
        let zero = term
            .high
            .eq(0.0)
            .and(self.low.to_bits().ne((-0.0_f64).to_bits()));
        factor.high.abs().le(0.75).and(zero.or(absorbed))
    }

    pub(super) fn neg(&self) -> Self {
        Self {
            high: self.high.neg(),
            low: self.low.neg(),
        }
    }

    pub(super) fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }

    pub(super) fn mul(&self, other: &Self) -> Self {
        // Dekker's split gives an exact residual for the leading product.
        // Callers keep these coefficients bounded, avoiding split overflow.
        let split = |value: &Val<F64>| {
            let expanded = value.mul(134_217_729.0);
            let high = expanded.sub(expanded.sub(value));
            let low = value.sub(&high);
            (high, low)
        };
        let product = self.high.mul(&other.high);
        let (left_high, left_low) = split(&self.high);
        let (right_high, right_low) = split(&other.high);
        let residual = left_high
            .mul(&right_high)
            .sub(&product)
            .add(left_high.mul(&right_low))
            .add(left_low.mul(&right_high))
            .add(left_low.mul(right_low))
            .add(self.high.mul(&other.low))
            .add(self.low.mul(&other.high))
            .add(self.low.mul(&other.low));
        Self::sum(product, residual)
    }

    pub(super) fn divide(&self, divisor: &Self) -> Self {
        let estimate = self.high.div(&divisor.high);
        let product = Self::from_f64(estimate.clone()).mul(divisor);
        let residual = self.sub(&product);
        Self::sum(estimate, residual.high.add(residual.low).div(&divisor.high))
    }

    /// Exact scaling within the binary64 normal range used by these kernels.
    pub(super) fn scale(&self, exponent: &Val<I32>) -> Self {
        let power = Val::<F64>::from_bits(exponent.add(1023).unsigned().extend::<I64>().shl(52));
        Self {
            high: self.high.mul(&power),
            low: self.low.mul(power),
        }
    }

    pub(super) fn select(&self, condition: &Val<I1>, other: &Self) -> Self {
        Self {
            high: condition.select(&self.high, &other.high),
            low: condition.select(&self.low, &other.low),
        }
    }

    /// Converts a positive coefficient into binary80 rounding evidence.
    /// The normalized pair must have 2^-128 <= high <= 2^128 and
    /// |low| <= 0.5 ulp(high), keeping the scaled residual within i32 range.
    /// `discarded_tail` means there are nonzero bits below this approximation;
    /// forcing the precision exception alone belongs on the rounded result.
    pub(super) fn magnitude(
        &self,
        exponent: Val<I32>,
        negative: Val<I1>,
        discarded_tail: Val<I1>,
    ) -> FiniteMagnitude {
        let bits = self.high.to_bits();
        let coefficient_exponent = bits.unsigned().shr(52).truncate::<I32>().sub(1023);
        let integer = bits.and((1_u64 << 52) - 1).or(1_u64 << 52).shl(11);
        let scale = Val::<F64>::from_bits(
            Val::<I32>::from(1086)
                .sub(&coefficient_exponent)
                .unsigned()
                .extend::<I64>()
                .shl(52),
        );
        let correction = self.low.mul(scale);
        let floor = correction.floor();
        let integer = integer.add(floor.trunc_saturating_i32().signed().extend::<I64>());
        let fraction = correction.sub(floor);
        // A negative residual below a power of two crosses into the lower binade.
        let lower_binade = integer.unsigned().lt(1_u64 << 63);
        let upper_half = fraction.ge(0.5);
        let integer = lower_binade.select(
            integer.shl(1).or(upper_half.unsigned().extend::<I64>()),
            integer,
        );
        let fraction =
            lower_binade.select(fraction.mul(2.0).sub(upper_half.select(1.0, 0.0)), fraction);
        FiniteMagnitude {
            significand: RoundingInput {
                integer,
                guard: fraction.ge(0.5),
                sticky: discarded_tail.or(fraction.ne(0.0).and(fraction.ne(0.5))),
            },
            exponent: exponent
                .add(coefficient_exponent)
                .sub(lower_binade.unsigned().extend::<I32>()),
            negative,
        }
    }
}

/// Sum 1 + q/3 + q²/5 + ... with a fixed, caller-bounded term count.
/// Positive q serves atanh; negative q serves atan. Consumers bound both
/// truncation and arithmetic error for their argument interval.
pub(super) fn odd_power_series(
    body: &mut BlockBuilder<'_>,
    square: &DoubleDouble,
    terms: u32,
) -> Result<DoubleDouble, BuildError> {
    let parts = body.loop_::<(I32, DoubleShape, DoubleShape), DoubleShape>(
        (3, (1.0, 0.0), (1.0, 0.0)),
        |mut iteration, labels, (index, term, sum)| {
            let term = DoubleDouble::from_parts(term);
            let sum = DoubleDouble::from_parts(sum);
            iteration.if_(
                index
                    .eq(2 * terms + 1)
                    .or(sum.absorbs_series_tail(&term, square)),
                |done| done.branch(&labels.exit, sum.parts()),
            )?;
            let term = term.mul(square);
            let sum = sum.add(&term.divide(&DoubleDouble::from_f64(index.unsigned().to_f64())));
            iteration.branch(&labels.again, (index.add(2), term.parts(), sum.parts()))
        },
    )?;
    Ok(DoubleDouble::from_parts(parts))
}
