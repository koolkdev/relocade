//! Exact alignment and cancellation precede the shared precision/range rounding.

use wasm86_compiler::{BlockBuilder, BuildError, Results, Val, I1, I32, I64, I8};

use super::{
    operand::{BinaryOperands, Operand},
    result::{BinaryArithmetic, RoundedValue},
    rounding::{FiniteMagnitude, RoundingInput},
    BinaryOperation, RoundingMode,
};

const LEADING: u64 = 1 << 63;

pub(super) fn add(
    body: &mut BlockBuilder<'_>,
    operands: &BinaryOperands,
    operation: BinaryOperation,
    precision: Val<I8>,
    rounding: &RoundingMode,
) -> Result<BinaryArithmetic, BuildError> {
    let BinaryOperands { left, right, .. } = operands;
    // Only numerical signs change. NaN propagation still sees the original
    // destination and source, even for reverse subtraction.
    let left_negative = left
        .bits
        .negative()
        .xor(matches!(operation, BinaryOperation::ReverseSubtract));
    let right_negative = right
        .bits
        .negative()
        .xor(matches!(operation, BinaryOperation::Subtract));
    let subtract_magnitudes = left_negative.xor(&right_negative);
    let aligned = AlignedMagnitudes::new(body, left, right)?;
    let negative = aligned.left_larger.select(&left_negative, &right_negative);
    let magnitude = aligned.calculate(body, &subtract_magnitudes, negative)?;
    let exact_zero = magnitude.significand.integer.eq(0_u64);
    let zero = RoundedValue::zero(
        subtract_magnitudes.select(rounding.cancellation_negative(), left_negative.clone()),
    );
    let mut result = magnitude.round(precision, rounding);
    result.replace_when(&exact_zero, &zero);
    let infinity_negative = left.infinity.select(left_negative, right_negative);
    result.replace_when(
        &left.infinity.or(&right.infinity),
        &RoundedValue::infinity(infinity_negative),
    );
    let invalid_operation = left.infinity.and(&right.infinity).and(subtract_magnitudes);
    Ok(BinaryArithmetic {
        result: operands.finish(result, invalid_operation),
        operands_valid: operands.precision_only(operation),
        round_magnitude: exact_zero.eq(false),
        zero,
    })
}

/// Sorting bounds cancellation and lets both operations share one alignment.
struct AlignedMagnitudes {
    larger: Val<I64>,
    smaller: Val<I64>,
    exponent: Val<I32>,
    distance: Val<I32>,
    integer: Val<I64>,
    fraction: Val<I64>,
    left_larger: Val<I1>,
}

impl AlignedMagnitudes {
    fn new(
        body: &mut BlockBuilder<'_>,
        left: &Operand,
        right: &Operand,
    ) -> Result<Self, BuildError> {
        let (left_significand, left_exponent) = left.normalized();
        let (right_significand, right_exponent) = right.normalized();
        // Zero sorts below every nonzero finite magnitude, including the smallest
        // subnormal. Its encoded exponent alone would not establish that order.
        let left_exponent = left.zero.select(-16446, left_exponent);
        let right_exponent = right.zero.select(-16446, right_exponent);
        let left_larger = right_exponent.signed().lt(&left_exponent).or(left_exponent
            .eq(&right_exponent)
            .and(left_significand.unsigned().ge(&right_significand)));
        let larger = left_larger.select(&left_significand, &right_significand);
        let smaller = left_larger.select(right_significand, left_significand);
        let exponent = left_larger.select(&left_exponent, &right_exponent);
        let distance = exponent.sub(left_larger.select(right_exponent, left_exponent));
        let (integer, fraction) = align(body, &smaller, &distance)?;
        Ok(Self {
            larger,
            smaller,
            exponent,
            distance,
            integer,
            fraction,
            left_larger,
        })
    }

    fn calculate(
        &self,
        body: &mut BlockBuilder<'_>,
        subtract: &Val<I1>,
        negative: Val<I1>,
    ) -> Result<FiniteMagnitude, BuildError> {
        let magnitude = UnsignedMagnitude::from_components(body.if_value::<MagnitudeShape>(
            subtract,
            |mut body| {
                let difference = self.difference(&mut body)?;
                body.yield_(difference.components())
            },
            |body| body.yield_(self.sum().components()),
        )?);
        Ok(FiniteMagnitude {
            significand: magnitude.significand,
            exponent: magnitude.exponent,
            negative,
        })
    }

    fn sum(&self) -> UnsignedMagnitude {
        let sum = self.larger.add(&self.integer);
        let carry = sum.unsigned().lt(&self.larger);
        UnsignedMagnitude {
            significand: RoundingInput {
                integer: carry.select(sum.unsigned().shr(1).or(LEADING), &sum),
                guard: carry.select(
                    sum.and(1_u64).ne(0_u64),
                    self.fraction.and(LEADING).ne(0_u64),
                ),
                sticky: carry.select(
                    self.fraction.ne(0_u64),
                    self.fraction.and(LEADING - 1).ne(0_u64),
                ),
            },
            exponent: self.exponent.add(carry.unsigned().extend::<I32>()),
        }
    }

    fn difference(&self, body: &mut BlockBuilder<'_>) -> Result<UnsignedMagnitude, BuildError> {
        let integer = self
            .larger
            .sub(&self.integer)
            .sub(self.fraction.ne(0_u64).unsigned().extend::<I64>());
        let remainder = Val::<I64>::from(0_u64).sub(&self.fraction);
        let renormalize = integer.unsigned().lt(LEADING);
        // Deep cancellation occurs only for gaps zero and one, where the exact
        // difference is an integer at the smaller operand's exponent.
        let close = self.distance.unsigned().lt(2).and(&renormalize);
        let exact = self.larger.shl(&self.distance).sub(&self.smaller);
        let shift = exact.clz().truncate::<I32>();
        let far_integer =
            renormalize.select(integer.shl(1).or(remainder.unsigned().shr(63)), integer);
        let far_fraction = renormalize.select(remainder.shl(1), remainder);
        Ok(UnsignedMagnitude::from_components(
            body.if_value::<MagnitudeShape>(
                close,
                |body| {
                    body.yield_((
                        exact.shl(&shift),
                        false,
                        false,
                        self.exponent.sub(self.distance.add(shift)),
                    ))
                },
                |body| {
                    body.yield_((
                        far_integer,
                        far_fraction.and(LEADING).ne(0_u64),
                        far_fraction.and(LEADING - 1).ne(0_u64),
                        self.exponent.sub(renormalize.unsigned().extend::<I32>()),
                    ))
                },
            )?,
        ))
    }
}

struct UnsignedMagnitude {
    significand: RoundingInput,
    exponent: Val<I32>,
}

type MagnitudeShape = (I64, I1, I1, I32);

impl UnsignedMagnitude {
    fn from_components(components: <MagnitudeShape as Results>::Values) -> Self {
        let (integer, guard, sticky, exponent) = components;
        Self {
            significand: RoundingInput {
                integer,
                guard,
                sticky,
            },
            exponent,
        }
    }

    fn components(self) -> <MagnitudeShape as Results>::Values {
        (
            self.significand.integer,
            self.significand.guard,
            self.significand.sticky,
            self.exponent,
        )
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
