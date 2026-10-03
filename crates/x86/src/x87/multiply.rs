//! Exact significand multiplication and x87 operand-class responses.

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I64, I8};

use super::{
    arithmetic::{
        ArithmeticCandidate, ArithmeticResult, FiniteMagnitude, RoundedShape, RoundedValue,
    },
    rounding::RoundingInput,
    value::Classification,
    ExtendedBits, ExtendedValue, RoundingMode,
};

struct Operand {
    bits: ExtendedBits,
    exponent: Val<I32>,
    zero: Val<I1>,
    infinity: Val<I1>,
    nan: Val<I1>,
    signaling_nan: Val<I1>,
    denormal: Val<I1>,
    unsupported: Val<I1>,
}

pub(crate) struct Multiplication {
    pub(crate) result: ArithmeticResult,
    normal_operands: Val<I1>,
    zero_product: Val<I1>,
    /// Valid only when at least one operand is zero and both are normal or zero.
    /// A subnormal partner still requires the denormal-operand response.
    zero: RoundedValue,
}

impl Multiplication {
    /// Proposes a rounded result without committing effects or choosing a fallback.
    /// Accepts two normal operands with an in-range product, or zero multiplied
    /// by a normal value or another zero. Other cases need the full arithmetic response.
    /// When valid, operand and range exceptions are excluded; inexact rounding
    /// (#P) and rounding direction (C1) remain part of the result.
    pub(crate) fn rounding_candidate(
        &self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<ArithmeticCandidate, BuildError> {
        let (valid, components) = body.if_value::<(I1, RoundedShape)>(
            &self.normal_operands,
            |body| {
                body.yield_((
                    &self.result.in_range.valid,
                    self.result.in_range.rounded.components(),
                ))
            },
            |body| body.yield_((&self.zero_product, self.zero.components())),
        )?;
        Ok(ArithmeticCandidate {
            valid,
            rounded: RoundedValue::from_components(
                components,
                Classification::normal().select(&self.normal_operands, &Classification::zero()),
            ),
        })
    }
}

impl Operand {
    fn new(value: &ExtendedValue) -> Self {
        let bits = value.bits();
        let exponent = bits.exponent_field();
        Self {
            zero: value.zero(),
            infinity: value.infinity(),
            signaling_nan: value.signaling_nan(),
            denormal: value.denormal(),
            unsupported: value.unsupported(),
            bits,
            exponent,
            nan: value.nan(),
        }
    }

    fn normalized(&self) -> (Val<I64>, Val<I32>) {
        // Only denormal operands need normalization. Zero and special responses
        // ignore this finite magnitude, including its exponent.
        let shift = self
            .denormal
            .select(self.bits.significand.clz().truncate::<I32>(), 0);
        let exponent = self.denormal.select(1, &self.exponent);
        (
            self.bits.significand.shl(&shift),
            exponent.sub(16383).sub(shift),
        )
    }
}

pub(crate) fn multiply(
    left: &ExtendedValue,
    right: &ExtendedValue,
    precision: Val<I8>,
    rounding: &RoundingMode,
) -> Multiplication {
    const LEADING: u64 = 1 << 63;
    let normal_operands = left.normal().and(right.normal());
    let zero_product = left
        .zero()
        .and(right.normal().or(right.zero()))
        .or(left.normal().and(right.zero()));
    let left = Operand::new(left);
    let right = Operand::new(right);
    let negative = left.bits.negative().xor(right.bits.negative());
    let zero_result = RoundedValue {
        value: ExtendedValue::from_bits(ExtendedBits {
            significand: 0_u64.into(),
            sign_exponent: negative.select(0x8000_u32, 0_u32),
        })
        .assume_class(Classification::zero()),
        inexact: false.into(),
        incremented: false.into(),
    };
    let (left_significand, left_exponent) = left.normalized();
    let (right_significand, right_exponent) = right.normalized();
    let (low, high) = left_significand.unsigned().mul_wide(right_significand);
    let upper_binade = high.and(LEADING).ne(0_u64);
    let integer = upper_binade.select(&high, high.shl(1).or(low.unsigned().shr(63)));
    let fraction = upper_binade.select(&low, low.shl(1));
    let magnitude = FiniteMagnitude {
        significand: RoundingInput {
            integer,
            guard: fraction.and(LEADING).ne(0_u64),
            sticky: fraction.and(LEADING - 1).ne(0_u64),
        },
        exponent: left_exponent
            .add(right_exponent)
            .add(upper_binade.unsigned().extend::<I32>()),
        negative: negative.clone(),
    };
    let mut result = magnitude.round(precision, rounding);

    let unsupported = left.unsupported.or(&right.unsupported);
    let nan = left.nan.or(&right.nan);
    let infinity = left.infinity.or(&right.infinity);
    let zero = left.zero.or(&right.zero);
    let indefinite = unsupported.or(infinity.and(&zero));
    let invalid = indefinite.or(left.signaling_nan.or(&right.signaling_nan));
    let finite = nan.or(&indefinite).or(&infinity).or(&zero).eq(false);

    // A QNaN wins over an SNaN. For equal classes, the larger significand wins;
    // equal-significand ties retain the destination operand as local policy.
    let left_nan = left.nan.and(
        right
            .nan
            .eq(false)
            .or(left.bits.significand.unsigned().ge(&right.bits.significand)),
    );
    let nan_significand = left_nan.select(&left.bits.significand, &right.bits.significand);
    let nan_sign = left_nan.select(&left.bits.sign_exponent, &right.bits.sign_exponent);
    let special_value = ExtendedValue::from_bits(ExtendedBits {
        significand: nan.select(
            nan_significand.or(1_u64 << 62),
            infinity.select(LEADING, 0_u64),
        ),
        sign_exponent: nan.select(
            nan_sign,
            negative
                .select::<I16>(0x8000, 0)
                .or(infinity.select(0x7fff, 0)),
        ),
    });
    let special = RoundedValue {
        value: special_value.or_indefinite(&indefinite),
        inexact: false.into(),
        incremented: false.into(),
    };
    result.masked = result.masked.select(&finite, &special);
    result.adjusted = result.adjusted.select(&finite, &special);
    result.in_range.rounded = result.in_range.rounded.select(&finite, &special);
    result.in_range.valid = finite.select(result.in_range.valid, true);
    result.overflow = finite.and(result.overflow);
    result.tiny = finite.and(result.tiny);
    result.denormal = invalid
        .or(nan)
        .eq(false)
        .and(left.denormal.or(right.denormal));
    result.invalid = invalid;
    Multiplication {
        result,
        normal_operands,
        zero_product,
        zero: zero_result,
    }
}
