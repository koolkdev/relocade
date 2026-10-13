//! A native estimate followed by exact correction supplies all 64 root bits.

use crate::floating::RoundingInput;
use wasm86_compiler::{BlockBuilder, BuildError, Val, F64, I1, I32, I64};

fn at_least(
    low: &Val<I64>,
    high: &Val<I64>,
    other_low: &Val<I64>,
    other_high: &Val<I64>,
) -> Val<I1> {
    other_high
        .unsigned()
        .lt(high)
        .or(high.eq(other_high).and(low.unsigned().ge(other_low)))
}

/// The normalized input has its leading bit at 63. The radicand is
/// S * 2^(63+odd); its root is a normalized 64-bit significand.
pub(super) fn calculate(
    body: &mut BlockBuilder<'_>,
    significand: &Val<I64>,
    odd: &Val<I1>,
) -> Result<RoundingInput, BuildError> {
    let coefficient = Val::<F64>::from_bits(
        significand
            .unsigned()
            .shr(11)
            .and((1_u64 << 52) - 1)
            .or(1023_u64 << 52),
    );
    let estimate = odd.select(coefficient.mul(2.0), coefficient).sqrt();
    let seed = estimate.to_bits().and((1_u64 << 52) - 1).or(1_u64 << 52);

    // Truncating S to 53 bits and rounding the native root leaves the correct
    // 53-bit floor within one of the seed. Compare squares against N/2^22,
    // where even (seed+1)^2 fits; the full-size next square could equal 2^128.
    let scaled_low = significand.shl(odd.select(42, 41));
    let scaled_high = significand.unsigned().shr(odd.select(22, 23));
    let (square_low, square_high) = seed.unsigned().mul_wide(&seed);
    let next_low = square_low.add(seed.shl(1).add(1_u64));
    let next_high = square_high.add(
        next_low
            .unsigned()
            .lt(&square_low)
            .unsigned()
            .extend::<I64>(),
    );
    let previous_delta = seed.shl(1).sub(1_u64);
    let previous_low = square_low.sub(&previous_delta);
    let previous_high = square_high.sub(
        square_low
            .unsigned()
            .lt(previous_delta)
            .unsigned()
            .extend::<I64>(),
    );
    let down = at_least(&scaled_low, &scaled_high, &square_low, &square_high).eq(false);
    let up = at_least(&scaled_low, &scaled_high, &next_low, &next_high);
    let prefix = seed
        .sub(down.unsigned().extend::<I64>())
        .add(up.unsigned().extend::<I64>());
    let square_low = down.select(previous_low, up.select(next_low, square_low));
    let square_high = down.select(previous_high, up.select(next_high, square_high));

    let radicand_low = odd.select(0_u64, significand.shl(63));
    let radicand_high = odd.select(significand, significand.unsigned().shr(1));
    let product_low = square_low.shl(22);
    let product_high = square_high.shl(22).or(square_low.unsigned().shr(42));
    let remainder_low = radicand_low.sub(&product_low);
    let remainder_high = radicand_high.sub(product_high).sub(
        radicand_low
            .unsigned()
            .lt(product_low)
            .unsigned()
            .extend::<I64>(),
    );

    let (integer, remainder_low, remainder_high) = body
        .loop_::<(I32, I64, I64, I64), (I64, I64, I64)>(
            (11, prefix.shl(11), remainder_low, remainder_high),
            |mut iteration, labels, (remaining, root, low, high)| {
                iteration.if_(remaining.eq(0), |done| {
                    done.branch(&labels.exit, (&root, &low, &high))
                })?;
                let bit = Val::<I64>::from(1_u64).shl(remaining.sub(1));
                // The root's lowest `remaining` bits are zero, so the terms in
                // 2*root*bit + bit^2 do not overlap and OR is exact.
                let delta_low = root.shl(&remaining).or(bit.shl(remaining.sub(1)));
                let delta_high = root.unsigned().shr(Val::<I32>::from(64).sub(&remaining));
                let take = at_least(&low, &high, &delta_low, &delta_high);
                let difference_high = high
                    .sub(delta_high)
                    .sub(low.unsigned().lt(&delta_low).unsigned().extend::<I64>());
                iteration.branch(
                    &labels.again,
                    (
                        remaining.sub(1),
                        take.select(root.or(bit), root),
                        take.select(low.sub(delta_low), low),
                        take.select(difference_high, high),
                    ),
                )
            },
        )?;
    // For integer N, sqrt(N) cannot equal R+1/2. Comparing the integer
    // remainder with R therefore decides the midpoint without a wide multiply.
    Ok(RoundingInput {
        guard: remainder_high
            .ne(0_u64)
            .or(integer.unsigned().lt(&remainder_low)),
        sticky: remainder_high.or(remainder_low).ne(0_u64),
        integer,
    })
}
