//! Exact binary32/64 expansion and directly rounded narrowing of extended values.

use wasm86_compiler::{Val, I1, I16, I32, I64};

use super::{
    rounding::{RoundingInput, RoundingMode},
    ExtendedBits, ExtendedValue,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum BinaryFormat {
    Binary32,
    Binary64,
}

pub(crate) struct BinaryOperand {
    pub(crate) value: ExtendedValue,
    pub(crate) signaling_nan: Val<I1>,
    pub(crate) denormal: Val<I1>,
}

/// Conversion evidence is independent of exception masks and stack occupancy.
pub(crate) struct BinaryResult {
    pub(crate) bits: Val<I64>,
    pub(crate) invalid: Val<I1>,
    pub(crate) overflow: Val<I1>,
    pub(crate) tiny: Val<I1>,
    pub(crate) inexact: Val<I1>,
    pub(crate) incremented: Val<I1>,
}

impl BinaryFormat {
    fn fraction_bits(self) -> u32 {
        match self {
            Self::Binary32 => 23,
            Self::Binary64 => 52,
        }
    }

    fn exponent_bits(self) -> u32 {
        match self {
            Self::Binary32 => 8,
            Self::Binary64 => 11,
        }
    }

    fn bias(self) -> u32 {
        (1 << (self.exponent_bits() - 1)) - 1
    }

    fn infinity(self) -> u64 {
        ((1_u64 << self.exponent_bits()) - 1) << self.fraction_bits()
    }

    fn quiet_bit(self) -> u64 {
        1 << (self.fraction_bits() - 1)
    }

    fn sign_bit(self) -> u64 {
        1 << (self.fraction_bits() + self.exponent_bits())
    }

    pub(super) fn indefinite_bits(self) -> u64 {
        self.sign_bit() | self.infinity() | self.quiet_bit()
    }

    fn denormal(self, bits: &Val<I64>) -> Val<I1> {
        let magnitude = bits.and(self.sign_bit() - 1);
        magnitude
            .ne(0_u64)
            .and(magnitude.unsigned().lt(1_u64 << self.fraction_bits()))
    }

    pub(super) fn tag(self, bits: &Val<I64>) -> Val<I16> {
        let magnitude = bits.and(self.sign_bit() - 1);
        // Narrow subnormals expand to normal extended values.
        magnitude.eq(0_u64).select(
            1_u32,
            magnitude
                .unsigned()
                .ge(self.infinity())
                .select(2_u32, 0_u32),
        )
    }

    pub(crate) fn bytes(self) -> u32 {
        match self {
            Self::Binary32 => 4,
            Self::Binary64 => 8,
        }
    }

    /// The candidate quiets an SNaN, but its exception is resolved with the
    /// destination's stack fault before deciding whether to commit the load.
    pub(crate) fn decode(self, bits: &Val<I64>) -> BinaryOperand {
        let nan = bits
            .and(self.sign_bit() - 1)
            .unsigned()
            .ge(self.infinity() + 1);
        let signaling_nan = nan.and(bits.and(self.quiet_bit()).eq(0_u64));
        BinaryOperand {
            value: ExtendedValue::from_binary(
                self,
                bits.or(signaling_nan.select(self.quiet_bit(), 0_u64)),
            ),
            signaling_nan,
            denormal: self.denormal(bits),
        }
    }

    /// Expands post-load bits exactly; decode has already quieted any SNaN.
    pub(super) fn expand(self, bits: &Val<I64>) -> ExtendedBits {
        let fraction_bits = self.fraction_bits();
        let exponent_bits = self.exponent_bits();
        let bias = self.bias();
        let exponent_mask = (1_u64 << exponent_bits) - 1;
        let fraction = bits.and((1_u64 << fraction_bits) - 1);
        let exponent = bits.unsigned().shr(fraction_bits).and(exponent_mask);
        let sign = bits
            .unsigned()
            .shr(fraction_bits + exponent_bits)
            .truncate::<I16>()
            .shl(15);
        let zero_exponent = exponent.eq(0_u64);
        let special = exponent.eq(exponent_mask);
        let nonzero_fraction = fraction.ne(0_u64);
        let fraction = fraction.shl(63 - fraction_bits);

        // Narrow subnormals are normal extended values. This exact expansion
        // uses neither the precision control nor the rounding control fields.
        let shift = fraction.clz();
        let significand = zero_exponent.select(
            fraction.shl(shift.truncate::<I32>()),
            fraction.or(1_u64 << 63),
        );
        let finite_exponent = zero_exponent.select(
            nonzero_fraction.select(Val::<I64>::from((16384 - bias) as u64).sub(shift), 0_u64),
            exponent.add((16383 - bias) as u64),
        );
        let sign_exponent = sign.or(special
            .select(0x7fff_u64, finite_exponent)
            .truncate::<I16>());
        ExtendedBits {
            significand,
            sign_exponent,
        }
    }

    /// Rounds the original extended value directly to the destination. PC is
    /// irrelevant to stores; no intermediate binary64 value is constructed.
    pub(crate) fn encode(self, value: &ExtendedValue, rounding: &RoundingMode) -> BinaryResult {
        if let Some(bits) = value.exact_bits(self) {
            return BinaryResult {
                bits: bits.clone(),
                invalid: false.into(),
                overflow: false.into(),
                // Exact subnormals still raise underflow when UM is clear.
                tiny: self.denormal(bits),
                inexact: false.into(),
                incremented: false.into(),
            };
        }
        let value = value.bits();
        let fraction_bits = self.fraction_bits();
        let precision = fraction_bits + 1;
        let shift = 64 - precision;
        let infinity = self.infinity();
        let quiet_bit = self.quiet_bit();
        let sign_bit = self.sign_bit();

        let significand = &value.significand;
        let exponent = value.sign_exponent.and(0x7fff).unsigned().extend::<I32>();
        let negative = value.sign_exponent.and(0x8000).ne(0);
        let sign = negative.select(sign_bit, 0_u64);
        let unsupported = exponent.ne(0).and(significand.and(1_u64 << 63).eq(0_u64));
        let special = exponent.eq(0x7fff);
        let nan = special.and(significand.and(0x7fff_ffff_ffff_ffff_u64).ne(0_u64));
        let signaling_nan = nan.and(significand.and(1_u64 << 62).eq(0_u64));
        let finite = unsupported.or(&special).eq(false);

        // E=0 encodings use the same exponent as E=1, including pseudo-denormals.
        // Every nonzero extended subnormal is far below either narrow format.
        let exponent = exponent.eq(0).select(1_u32, exponent);
        let minimum_exponent = 16384 - self.bias();
        let precision_rounded =
            rounding.round(RoundingInput::shift_right(significand, shift), &negative);
        let carry = precision_rounded.integer.eq(1_u64 << precision);
        let rounded_exponent = exponent.add(carry.unsigned().extend::<I32>());
        // Intel tests tininess after precision rounding with an unbounded
        // exponent. A masked store can round to minimum normal and still be tiny.
        let tiny = finite
            .and(significand.ne(0_u64))
            .and(rounded_exponent.unsigned().lt(minimum_exponent));
        let overflow = finite.and(rounded_exponent.unsigned().ge(16384 + self.bias()));

        // The subnormal grid is rounded from the original bits, never from the
        // precision-rounded candidate used to detect range exceptions.
        let below_normal = exponent.unsigned().lt(minimum_exponent);
        let distance = below_normal.select(
            Val::<I32>::from(minimum_exponent + shift).sub(&exponent),
            shift,
        );
        let stored = rounding.round(RoundingInput::shift_right(significand, distance), &negative);
        // Adding the retained leading bit also propagates a rounding carry.
        let exponent_field = below_normal.select(0_u32, exponent.sub(minimum_exponent));
        let finite_bits = exponent_field
            .unsigned()
            .extend::<I64>()
            .shl(fraction_bits)
            .add(&stored.integer);
        let to_infinity = rounding.overflow_to_infinity(&negative);
        let finite_bits = overflow.select(to_infinity.select(infinity, infinity - 1), finite_bits);
        let nan_bits = significand
            .unsigned()
            .shr(shift)
            .and((1_u64 << fraction_bits) - 1)
            .or(quiet_bit);
        let special_bits = nan.select(nan_bits, 0_u64).or(infinity);
        let bits = unsupported.select(
            sign_bit | infinity | quiet_bit,
            sign.or(special.select(special_bits, finite_bits)),
        );
        BinaryResult {
            bits,
            invalid: unsupported.or(signaling_nan),
            overflow: overflow.clone(),
            tiny,
            inexact: finite.and(overflow.or(&stored.inexact)),
            incremented: finite.and(overflow.select(to_infinity, stored.incremented)),
        }
    }
}
