//! Unrounded finite magnitudes and binary32/64 rounding, independent of exception policy.

use super::{BinaryEncoding, BinaryFormat, RoundingInput, RoundingMode};
use wasm86_compiler::{AtLeast, Results, Val, I1, I32, I64};

/// The leading significand bit is bit 63; zero has no significant bits.
/// The signed exponent is unbiased, and the fraction has not yet been rounded.
/// Unnormalized inputs are also valid when far below the destination's range.
pub(crate) struct FiniteMagnitude {
    pub(crate) significand: RoundingInput,
    pub(crate) exponent: Val<I32>,
    pub(crate) negative: Val<I1>,
}

pub(crate) type MagnitudeShape = (I64, I1, I1, I32, I1);

impl FiniteMagnitude {
    pub(crate) fn components(&self) -> <MagnitudeShape as Results>::Values {
        (
            self.significand.integer.clone(),
            self.significand.guard.clone(),
            self.significand.sticky.clone(),
            self.exponent.clone(),
            self.negative.clone(),
        )
    }

    pub(crate) fn from_components(
        (integer, guard, sticky, exponent, negative): <MagnitudeShape as Results>::Values,
    ) -> Self {
        Self {
            significand: RoundingInput {
                integer,
                guard,
                sticky,
            },
            exponent,
            negative,
        }
    }

    /// Records a negative tail too small to cross a nonzero remainder boundary.
    /// Requires a normalized, nonzero magnitude.
    /// The supplied rounding evidence must describe the value before that tail exactly.
    /// At an integer or half-integer boundary it changes the rounding evidence;
    /// at other fractions it only records inexactness. A borrow can cross a binade.
    pub(crate) fn with_negative_tail(mut self, tail: &Val<I1>) -> Self {
        let boundary = tail.and(self.significand.sticky.eq(false));
        let borrow = boundary.and(self.significand.guard.eq(false));
        let lower_binade = borrow.and(self.significand.integer.eq(1_u64 << 63));
        self.significand.integer = lower_binade.select(
            u64::MAX,
            self.significand
                .integer
                .sub(borrow.unsigned().extend::<I64>()),
        );
        self.significand.guard =
            boundary.select(self.significand.guard.eq(false), &self.significand.guard);
        self.significand.sticky = tail.or(&self.significand.sticky);
        self.exponent = self.exponent.sub(lower_binade.unsigned().extend::<I32>());
        self
    }
}

/// An unsigned encoding and raw rounding evidence for a finite value. Range
/// evidence uses precision rounding with an unbounded exponent; subnormal bits
/// are rounded independently from the original significand. Owners insert the
/// sign, choose overflow responses and interpret discarded bits according to
/// their exception policy. The magnitude is unused on overflow.
pub(crate) struct BinaryRounded<T: BinaryEncoding> {
    pub(crate) magnitude: Val<T>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(crate) bounded_inexact: Val<I1>,
    pub(crate) bounded_incremented: Val<I1>,
    pub(crate) unbounded_inexact: Val<I1>,
}

impl BinaryFormat {
    /// Normalizes a finite encoding without rounding. Special-operand responses
    /// must replace the unused numerical result for infinities and NaNs.
    pub(crate) fn finite_magnitude(self, bits: &Val<I64>) -> FiniteMagnitude {
        let fraction_bits = self.fraction_bits();
        let raw_exponent = self.exponent_field(bits);
        let significand = bits
            .and((1_u64 << fraction_bits) - 1)
            .or(raw_exponent.ne(0).select(1_u64 << fraction_bits, 0_u64));
        let leading = significand.clz().truncate::<I32>();
        FiniteMagnitude {
            significand: RoundingInput::exact(significand.shl(&leading)),
            exponent: raw_exponent
                .eq(0)
                .select(1, &raw_exponent)
                .sub(self.bias() as i32 - (63 - fraction_bits) as i32)
                .sub(leading),
            negative: match self {
                Self::Binary32 => bits.truncate::<I32>().signed().lt(0),
                Self::Binary64 => bits.signed().lt(0),
            },
        }
    }
}

impl FiniteMagnitude {
    /// The source classifier admits finite values for range checks. Other
    /// encodings produce no overflow/tiny evidence; their magnitude candidate
    /// and discarded-bit evidence must still be replaced by the caller.
    pub(crate) fn round_binary<T: BinaryEncoding>(
        &self,
        rounding: &RoundingMode,
        finite: &Val<I1>,
    ) -> BinaryRounded<T>
    where
        I64: AtLeast<T>,
    {
        let format = T::FORMAT;
        let fraction_bits = format.fraction_bits();
        let precision = fraction_bits + 1;
        let shift = 64 - precision;
        let minimum = 1 - format.bias() as i32;
        let unbounded = rounding.round(self.significand.shift_right(shift), &self.negative);
        let carry = unbounded.integer.eq(1_u64 << precision);
        let exponent = self.exponent.add(carry.unsigned().extend::<I32>());
        let nonzero = self
            .significand
            .integer
            .ne(0_u64)
            .or(&self.significand.guard)
            .or(&self.significand.sticky);
        let finite_nonzero = finite.and(&nonzero);
        let overflow = finite_nonzero.and(exponent.signed().ge(format.bias() as i32 + 1));
        let tiny = finite_nonzero.and(exponent.signed().lt(minimum));
        let below_normal = self.exponent.signed().lt(minimum);
        let distance = below_normal.select(
            Val::<I32>::from(minimum + shift as i32).sub(&self.exponent),
            shift,
        );
        // Never round an already precision-rounded value a second time.
        let bounded = rounding.round(self.significand.shift_right(distance), &self.negative);
        let exponent_field = below_normal.select(0, self.exponent.sub(minimum));
        let finite_bits = exponent_field
            .unsigned()
            .extend::<I64>()
            .truncate::<T>()
            .shl(fraction_bits)
            .add(bounded.integer.truncate::<T>());
        BinaryRounded {
            magnitude: nonzero.select(finite_bits, 0),
            overflow,
            tiny,
            bounded_inexact: bounded.inexact,
            bounded_incremented: bounded.incremented,
            unbounded_inexact: unbounded.inexact,
        }
    }
}

impl RoundingMode {
    /// The masked overflow response is infinity or the largest finite magnitude.
    pub(crate) fn overflow_magnitude<T: BinaryEncoding>(&self, negative: &Val<I1>) -> Val<T>
    where
        I64: AtLeast<T>,
    {
        self.overflow_to_infinity(negative).select(
            Val::<I64>::from(T::FORMAT.infinity()).truncate::<T>(),
            Val::<I64>::from(T::FORMAT.infinity() - 1).truncate::<T>(),
        )
    }
}
