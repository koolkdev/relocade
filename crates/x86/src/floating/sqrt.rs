//! A native estimate followed by exact correction supplies all 64 root bits.

use super::{FiniteInput, FiniteMagnitude, RoundingInput};

#[cfg(test)]
mod tests;
use relocade_compiler::{Val, F64, I1, I64};

/// The normalized input S has its leading bit at 63. The radicand
/// N = S * 2^(63+odd) has a normalized 64-bit integer square root.
fn calculate(significand: &Val<I64>, odd: &Val<I1>) -> RoundingInput {
    let coefficient = Val::<F64>::from_bits(
        significand
            .unsigned()
            .shr(11)
            .and((1_u64 << 52) - 1)
            .or(1023_u64 << 52),
    );
    let estimate = odd.select(coefficient.mul(2.0), coefficient).sqrt();
    let seed = estimate.to_bits().and((1_u64 << 52) - 1).or(1_u64 << 52);

    // For M = N / 2^22 the seed is within one of floor(sqrt(M)).
    // The difference M - seed² therefore fits in a signed word.
    let scaled_radicand_low = significand.shl(odd.select(42, 41));
    let error = scaled_radicand_low.sub(seed.mul(&seed));
    let twice_seed = seed.shl(1);
    let next_square_delta = twice_seed.add(1_u64);
    let down = error.signed().lt(0);
    let up = error.signed().ge(&next_square_delta);
    let prefix = seed
        .add(up.unsigned().extend::<I64>())
        .sub(down.unsigned().extend::<I64>());
    let prefix_remainder = down.select(
        error.add(twice_seed.sub(1_u64)),
        up.select(error.sub(next_square_delta), &error),
    );

    // For U = prefix * 2048, N - U² = prefix_remainder * 2^22.
    // Divide this by 2U to estimate the last eleven root bits. The
    // prefix is exact, so prefix_remainder <= 2*prefix. The dividend
    // fits in one word and the digit is at most 2048.
    let dividend = prefix_remainder.shl(10);
    let digit = dividend.unsigned().div(&prefix);
    let linear_remainder = dividend.sub(digit.mul(&prefix));
    let candidate = prefix.shl(11).add(&digit);

    // N - candidate² = 4096*linear_remainder - digit². A negative
    // residual has magnitude <= 2^22. Since candidate >= 2^63, the
    // true root then lies between candidate - 1/2 and candidate. Its floor
    // is candidate - 1, with both guard and sticky set. A nonnegative integer
    // residual places the root above the midpoint exactly when it exceeds candidate.
    let linear_low = linear_remainder.shl(12);
    let digit_square = digit.mul(&digit);
    let borrow = linear_low.unsigned().lt(&digit_square);
    let residual_high = linear_remainder
        .unsigned()
        .shr(52)
        .sub(borrow.unsigned().extend::<I64>());
    let residual_low = linear_low.sub(digit_square);
    RoundingInput {
        integer: candidate.sub(residual_high.signed().lt(0).unsigned().extend::<I64>()),
        guard: residual_high
            .ne(0_u64)
            .or(candidate.unsigned().lt(&residual_low)),
        sticky: residual_high.or(residual_low).ne(0_u64),
    }
}

impl FiniteInput {
    /// Supplies a positive root candidate for normalized, nonzero input.
    /// The architectural caller replaces zero and invalid-operand responses.
    pub(crate) fn square_root(&self) -> FiniteMagnitude {
        FiniteMagnitude {
            significand: calculate(&self.significand, &self.exponent.and(1).ne(0)),
            exponent: self.exponent.signed().shr(1),
            negative: false.into(),
        }
    }
}
