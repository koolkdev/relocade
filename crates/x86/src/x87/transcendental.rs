//! Extra significand precision for bounded transcendental calculations.
//!
//! Each value is the unevaluated sum of two binary64 components. Strict Wasm
//! arithmetic retains the addition/product residuals; the guest exponent stays
//! separate so the binary64 exponent range does not limit binary80 operands.

use wasm86_compiler::{BlockBuilder, BuildError, Val, F64, I1, I32, I64};

use crate::floating::{FiniteMagnitude, RoundingInput};

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
    let parts = body.loop_::<(I32, F64, F64, F64, F64), DoubleShape>(
        (3, 1.0, 0.0, 1.0, 0.0),
        |mut iteration, labels, (index, term_high, term_low, sum_high, sum_low)| {
            iteration.if_(index.eq(2 * terms + 1), |done| {
                done.branch(&labels.exit, (&sum_high, &sum_low))
            })?;
            let term = DoubleDouble::from_parts((term_high, term_low)).mul(square);
            let sum = DoubleDouble::from_parts((sum_high, sum_low))
                .add(&term.divide(&DoubleDouble::from_f64(index.unsigned().to_f64())));
            let (term_high, term_low) = term.parts();
            let (sum_high, sum_low) = sum.parts();
            iteration.branch(
                &labels.again,
                (index.add(2), term_high, term_low, sum_high, sum_low),
            )
        },
    )?;
    Ok(DoubleDouble::from_parts(parts))
}
