//! Exact reduction against the P4's 66-bit approximation of pi.

use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32, I64};

use super::super::transcendental::DoubleDouble;

pub(super) struct ReducedAngle {
    pub(super) magnitude: DoubleDouble,
    pub(super) negative: Val<I1>,
    pub(super) quadrant: Val<I32>,
}

/// For a normalized significand and -1 <= exponent <= 62, x/(pi/2)
/// is (significand << (exponent+2)) / 0x3243f6a8885a308d3.
/// Integer division preserves all remainder bits even near 2^63, where
/// subtracting a floating-point multiple of pi would lose the small angle.
pub(super) fn reduce(
    body: &mut BlockBuilder<'_>,
    significand: &Val<I64>,
    exponent: &Val<I32>,
) -> Result<ReducedAngle, BuildError> {
    const PI_LOW: u64 = 0x243f_6a88_85a3_08d3;
    // The significand is below the divisor. Append exponent+2 zero bits,
    // retaining a remainder below the divisor after every step. Common small
    // angles need only a few iterations; the full domain needs at most 64.
    let (high, low, quadrant) = body.loop_::<(I32, I64, I64, I32), (I64, I64, I32)>(
        (exponent.add(2), 0_u64, significand, 0),
        |mut iteration, labels, (remaining, high, low, quadrant)| {
            iteration.if_(remaining.eq(0), |done| {
                done.branch(&labels.exit, (&high, &low, &quadrant))
            })?;
            let high = high.shl(1).or(low.unsigned().shr(63));
            let low = low.shl(1);
            let subtract = high
                .unsigned()
                .ge(4_u64)
                .or(high.eq(3_u64).and(low.unsigned().ge(PI_LOW)));
            let borrow = low.unsigned().lt(PI_LOW);
            iteration.branch(
                &labels.again,
                (
                    remaining.sub(1),
                    high.sub(subtract.select(
                        Val::<I64>::from(3_u64).add(borrow.unsigned().extend::<I64>()),
                        0_u64,
                    )),
                    low.sub(subtract.select(PI_LOW, 0_u64)),
                    quadrant
                        .shl(1)
                        .or(subtract.unsigned().extend::<I32>())
                        .and(3),
                ),
            )
        },
    )?;
    // Choose the nearest multiple, leaving |angle| <= pi/4. The odd divisor
    // cannot have an integer half-remainder, so there is no tie policy here.
    let doubled_high = high.shl(1).or(low.unsigned().shr(63));
    let doubled_low = low.shl(1);
    let negative = doubled_high.unsigned().ge(4_u64).or(doubled_high
        .eq(3_u64)
        .and(doubled_low.unsigned().ge(PI_LOW)));
    let complement_low = Val::<I64>::from(PI_LOW).sub(&low);
    let complement_high = Val::<I64>::from(3_u64).sub(&high).sub(
        Val::<I64>::from(PI_LOW)
            .unsigned()
            .lt(&low)
            .unsigned()
            .extend::<I64>(),
    );
    let low = negative.select(complement_low, low);
    let high = negative.select(complement_high, high);
    let magnitude = DoubleDouble::significand(&low)
        .add(&DoubleDouble::from_f64(high.unsigned().to_f64().mul(2.0)))
        .scale(&(-2).into());
    Ok(ReducedAngle {
        magnitude,
        negative: negative.clone(),
        quadrant: quadrant.add(negative.unsigned().extend::<I32>()).and(3),
    })
}
