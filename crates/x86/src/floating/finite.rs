//! Unrounded finite magnitudes and binary32/64 rounding, independent of exception policy.

use super::{BinaryFormat, RoundingInput, RoundingMode};
use wasm86_compiler::{AtLeast, MemoryInt, Results, Val, I1, I32, I64};

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

/// Range evidence uses precision rounding with an unbounded exponent. The stored
/// bits are rounded independently on the destination's subnormal grid.
pub(crate) struct BinaryRounded {
    pub(crate) bits: Val<I64>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(crate) inexact: Val<I1>,
    pub(crate) incremented: Val<I1>,
    pub(crate) unbounded_inexact: Val<I1>,
}

impl BinaryFormat {
    /// Normalizes a finite encoding without rounding. Special-operand responses
    /// must replace the unused numerical result for infinities and NaNs.
    pub(crate) fn finite_magnitude(self, bits: &Val<I64>) -> FiniteMagnitude {
        let fraction_bits = self.fraction_bits();
        let raw_exponent = bits
            .unsigned()
            .shr(fraction_bits)
            .truncate::<I32>()
            .and((1_u32 << self.exponent_bits()) - 1);
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
    /// The source classifier excludes special encodings before range evidence
    /// is used for saturation. When `finite` is false, overflow/tiny are false;
    /// the caller still replaces result bits and filters rounding evidence.
    pub(crate) fn round_binary(
        &self,
        format: BinaryFormat,
        rounding: &RoundingMode,
        finite: &Val<I1>,
    ) -> BinaryRounded {
        match format {
            BinaryFormat::Binary32 => self.round_encoding::<I32>(format, rounding, finite),
            BinaryFormat::Binary64 => self.round_encoding::<I64>(format, rounding, finite),
        }
    }

    // Construct each encoding at its logical width. Keeping binary32 in I32 also
    // lets the compiler avoid carrying its bit manipulations through i64 locals.
    fn round_encoding<T: MemoryInt>(
        &self,
        format: BinaryFormat,
        rounding: &RoundingMode,
        finite: &Val<I1>,
    ) -> BinaryRounded
    where
        I64: AtLeast<T>,
    {
        let encode = |bits| Val::<I64>::from(bits).truncate::<T>();
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
        let normal_bits = self
            .exponent
            .sub(minimum)
            .unsigned()
            .extend::<I64>()
            .truncate::<T>()
            .shl(fraction_bits)
            .add(unbounded.integer.truncate::<T>());
        let finite_bits = below_normal.select(bounded.integer.truncate::<T>(), normal_bits);
        let to_infinity = rounding.overflow_to_infinity(&self.negative);
        let bits = overflow.select(
            to_infinity.select(encode(format.infinity()), encode(format.infinity() - 1)),
            finite_bits,
        );
        BinaryRounded {
            bits: nonzero
                .select(bits, encode(0))
                .or(self.negative.select(encode(format.sign_bit()), encode(0)))
                .unsigned()
                .extend::<I64>(),
            overflow: overflow.clone(),
            tiny,
            inexact: overflow.or(&bounded.inexact),
            incremented: overflow.select(to_infinity, bounded.incremented),
            unbounded_inexact: unbounded.inexact,
        }
    }
}
