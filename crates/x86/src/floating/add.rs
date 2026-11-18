//! Exact alignment and cancellation shared by binary32, binary64 and extended arithmetic.

use super::{FiniteInput, FiniteMagnitude, RoundingInput, RoundingMode};
use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32, I64};

const LEADING: u64 = 1 << 63;

impl FiniteInput {
    /// Exact zero preserves its rounding-dependent sign; its exponent is unspecified.
    pub(crate) fn add(
        &self,
        body: &mut BlockBuilder<'_>,
        right: &Self,
        rounding: &RoundingMode,
    ) -> Result<FiniteMagnitude, BuildError> {
        let subtract = self.negative.xor(&right.negative);
        let (left_significand, left_exponent) = (&self.significand, &self.exponent);
        let (right_significand, right_exponent) = (&right.significand, &right.exponent);
        // Zero sorts below every nonzero finite magnitude, including the smallest
        // subnormal. Its encoded exponent alone would not establish that order.
        let left_exponent = left_significand.eq(0_u64).select(-16446, left_exponent);
        let right_exponent = right_significand.eq(0_u64).select(-16446, right_exponent);
        let left_larger = right_exponent.signed().lt(&left_exponent).or(left_exponent
            .eq(&right_exponent)
            .and(left_significand.unsigned().ge(right_significand)));
        let larger = left_larger.select(left_significand, right_significand);
        let smaller = left_larger.select(right_significand, left_significand);
        let exponent = left_larger.select(&left_exponent, &right_exponent);
        let distance = exponent.sub(left_larger.select(right_exponent, left_exponent));
        let (integer, fraction) = align(body, &smaller, &distance)?;
        let aligned = AlignedMagnitudes {
            larger,
            exponent,
            integer,
            fraction,
        };
        let negative = left_larger.select(&self.negative, &right.negative);
        let mut magnitude = aligned.combine(body, &subtract, negative)?;
        let zero_negative = subtract.select(rounding.cancellation_negative(), &self.negative);
        magnitude.negative = magnitude
            .significand
            .integer
            .eq(0_u64)
            .select(zero_negative, magnitude.negative);
        Ok(magnitude)
    }

    /// Adds or subtracts a smaller significand whose exponent is 1..=65 below
    /// this operand's. Only gap 65 shifts a bit below the fractional word.
    pub(crate) fn add_smaller(
        self,
        body: &mut BlockBuilder<'_>,
        smaller: &Val<I64>,
        distance: &Val<I32>,
        subtract: &Val<I1>,
    ) -> Result<FiniteMagnitude, BuildError> {
        let integer = distance
            .unsigned()
            .lt(64)
            .select(smaller.unsigned().shr(distance), 0_u64);
        let fraction = distance.eq(65).select(
            smaller.unsigned().shr(1).or(smaller.and(1_u64)),
            smaller.shl(Val::<I32>::from(0).sub(distance)),
        );
        AlignedMagnitudes {
            larger: self.significand,
            exponent: self.exponent,
            integer,
            fraction,
        }
        .combine(body, subtract, self.negative)
    }
}

/// The smaller operand has been aligned into an integer and a fractional word.
struct AlignedMagnitudes {
    larger: Val<I64>,
    exponent: Val<I32>,
    integer: Val<I64>,
    fraction: Val<I64>,
}

impl AlignedMagnitudes {
    fn combine(
        &self,
        body: &mut BlockBuilder<'_>,
        subtract: &Val<I1>,
        negative: Val<I1>,
    ) -> Result<FiniteMagnitude, BuildError> {
        let (integer, fraction, exponent) = body.if_value::<(I64, I64, I32)>(
            subtract,
            |body| body.yield_(self.difference()),
            |body| body.yield_(self.sum()),
        )?;
        Ok(FiniteMagnitude {
            significand: RoundingInput {
                integer,
                guard: fraction.signed().lt(0),
                sticky: fraction.and(LEADING - 1).ne(0_u64),
            },
            exponent,
            negative,
        })
    }

    fn sum(&self) -> (Val<I64>, Val<I64>, Val<I32>) {
        let sum = self.larger.add(&self.integer);
        let carry = sum.unsigned().lt(&self.larger);
        let integer = carry.select(sum.unsigned().shr(1).or(LEADING), &sum);
        let fraction = carry.select(
            sum.shl(63)
                .or(self.fraction.ne(0_u64).unsigned().extend::<I64>()),
            &self.fraction,
        );
        (
            integer,
            fraction,
            self.exponent.add(carry.unsigned().extend::<I32>()),
        )
    }

    fn difference(&self) -> (Val<I64>, Val<I64>, Val<I32>) {
        let integer = self
            .larger
            .sub(&self.integer)
            .sub(self.fraction.ne(0_u64).unsigned().extend::<I64>());
        let fraction = Val::<I64>::from(0).sub(&self.fraction);
        let shift = integer.clz().truncate::<I32>();
        // When the integer vanishes, the difference is either zero or exactly
        // half a unit at gap one. A 64-bit shift then moves the entire fraction
        // into the significand. At wider gaps cancellation shifts at most once.
        let significand = integer.shl(&shift).or(shift.eq(0).select(
            0_u64,
            fraction.unsigned().shr(Val::<I32>::from(0).sub(&shift)),
        ));
        let remainder = shift.eq(64).select(0_u64, fraction.shl(&shift));
        (significand, remainder, self.exponent.sub(shift))
    }
}

/// Aligns into an integer and a fractional word. Beyond that word, lost bits
/// are jammed into its low bit. Such gaps allow at most one left shift during
/// cancellation, so this preserves every later guard and sticky decision.
fn align(
    body: &mut BlockBuilder<'_>,
    significand: &Val<I64>,
    distance: &Val<I32>,
) -> Result<(Val<I64>, Val<I64>), BuildError> {
    let below_word = distance.unsigned().lt(64);
    let near_fraction = distance
        .eq(0)
        .select(0_u64, significand.shl(Val::<I32>::from(64).sub(distance)));
    let beyond_word = distance.sub(64);
    let below_two_words = beyond_word.unsigned().lt(64);
    let far_fraction = below_two_words.select(significand.unsigned().shr(&beyond_word), 0_u64);
    let lost = below_two_words.select(
        significand
            .and(Val::<I64>::from(1_u64).shl(&beyond_word).sub(1_u64))
            .ne(0_u64),
        significand.ne(0_u64),
    );
    body.if_value::<(I64, I64)>(
        below_word,
        |body| body.yield_((significand.unsigned().shr(distance), near_fraction)),
        |body| body.yield_((0_u64, far_fraction.or(lost.unsigned().extend::<I64>()))),
    )
}
