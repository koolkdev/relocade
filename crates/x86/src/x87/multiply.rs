//! Exact significand multiplication and x87 operand-class responses.

use wasm86_compiler::{Val, I1, I16, I32, I64, I8};

use super::{
    arithmetic::{ArithmeticResult, FiniteMagnitude, RoundedValue},
    rounding::RoundingInput,
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
}

impl Operand {
    fn new(value: &ExtendedValue) -> Self {
        let bits = value.bits();
        let exponent = bits.exponent_field();
        let fraction = bits.significand.and(0x7fff_ffff_ffff_ffff_u64);
        let special = exponent.eq(0x7fff);
        let nan = special.and(fraction.ne(0_u64));
        Self {
            zero: exponent.eq(0).and(bits.significand.eq(0_u64)),
            infinity: special.and(fraction.eq(0_u64)),
            signaling_nan: nan.and(bits.significand.and(1_u64 << 62).eq(0_u64)),
            denormal: exponent.eq(0).and(bits.significand.ne(0_u64)),
            bits,
            exponent,
            nan,
        }
    }

    fn normalized(&self) -> (Val<I64>, Val<I32>) {
        let shift = self.bits.significand.clz().truncate::<I32>();
        let exponent = self.exponent.eq(0).select(1, &self.exponent);
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
) -> ArithmeticResult {
    const LEADING: u64 = 1 << 63;
    let left = Operand::new(left);
    let right = Operand::new(right);
    let negative = left.bits.negative().xor(right.bits.negative());
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

    let unsupported = left.bits.unsupported().or(right.bits.unsupported());
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
    result.overflow = finite.and(result.overflow);
    result.tiny = finite.and(result.tiny);
    result.denormal = invalid
        .or(nan)
        .eq(false)
        .and(left.denormal.or(right.denormal));
    result.invalid = invalid;
    result
}
