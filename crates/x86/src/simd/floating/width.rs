//! Width conversions preserve NaN payloads and distinguish precision from range.

use super::{FloatOperand, FloatResult, SimdExceptions};
use crate::{floating::RoundingInput, state::Mxcsr};
use wasm86_compiler::{Val, I32, I64};

impl FloatOperand<I32> {
    pub(crate) fn widen(&self) -> FloatResult<I64> {
        let bits = self.bits.unsigned().extend::<I64>();
        let nan = bits
            .and(0x8000_0000_u64)
            .shl(32)
            .or(bits.and(0x007f_ffff_u64).shl(29))
            .or(0x7ff8_0000_0000_0000_u64);
        FloatResult {
            value: self
                .classification()
                .nan()
                .select(nan, self.value().to_bits()),
            exceptions: SimdExceptions {
                invalid: self.classification().signaling_nan(),
                denormal: self.denormal.clone(),
                ..SimdExceptions::default()
            },
        }
    }
}

impl FloatOperand<I64> {
    pub(crate) fn narrow(&self, controls: &Mxcsr) -> FloatResult<I32> {
        let bits = &self.bits;
        let negative = bits.signed().lt(0_u64);
        let sign = bits
            .unsigned()
            .shr(32)
            .truncate::<I32>()
            .and(0x8000_0000_u32);
        let raw_exponent = bits.unsigned().shr(52).truncate::<I32>().and(0x7ff);
        let significand = bits
            .and(0x000f_ffff_ffff_ffff_u64)
            .or(raw_exponent.ne(0).select(1_u64 << 52, 0_u64));
        let leading = significand.clz().truncate::<I32>();
        let normalized = RoundingInput::exact(significand.shl(&leading));
        let exponent = raw_exponent
            .eq(0)
            .select(1, &raw_exponent)
            .sub(1012)
            .sub(&leading);
        let rounding = controls.rounding();

        // Range is checked after rounding to 24 bits with an unbounded exponent.
        // Bounded subnormal rounding must start from the original significand.
        let unbounded = rounding.round(normalized.shift_right(40), &negative);
        let rounded_exponent =
            exponent.add(unbounded.integer.eq(1_u64 << 24).unsigned().extend::<I32>());
        let finite_nonzero = raw_exponent.ne(0x7ff).and(significand.ne(0_u64));
        let overflow = finite_nonzero.and(rounded_exponent.signed().ge(128));
        let tiny = finite_nonzero.and(rounded_exponent.signed().lt(-126));
        let subnormal = exponent.signed().lt(-126);
        let distance = subnormal.select(Val::<I32>::from(-86_i32).sub(&exponent), 40);
        let bounded = rounding.round(normalized.shift_right(distance), &negative);
        let finite_bits = subnormal.select(
            bounded.integer.truncate::<I32>(),
            exponent
                .add(126)
                .shl(23)
                .add(unbounded.integer.truncate::<I32>()),
        );
        let overflow_bits = rounding
            .overflow_to_infinity(&negative)
            .select(0x7f80_0000_u32, 0x7f7f_ffff_u32);
        let flush = tiny
            .and(controls.underflow_masked())
            .and(controls.flush_to_zero());
        let finite_bits = flush.select(0, overflow.select(overflow_bits, finite_bits));
        let finite_bits = significand.eq(0_u64).select(0, finite_bits);
        let nan = bits
            .unsigned()
            .shr(29)
            .truncate::<I32>()
            .and(0x007f_ffff_u32)
            .or(0x7fc0_0000_u32);
        let result = raw_exponent
            .eq(0x7ff)
            .select(
                self.classification().nan().select(nan, 0x7f80_0000_u32),
                finite_bits,
            )
            .or(sign);

        // An unmasked range exception reports precision from unbounded rounding.
        // A masked one reports the final bounded result, including FTZ loss.
        let unmasked_range = overflow
            .and(controls.overflow_masked().eq(false))
            .or(tiny.and(controls.underflow_masked().eq(false)));
        let precision = finite_nonzero.and(
            unmasked_range.select(unbounded.inexact, overflow.or(&bounded.inexact).or(&flush)),
        );
        FloatResult {
            value: result,
            exceptions: SimdExceptions {
                invalid: self.classification().signaling_nan(),
                denormal: self.denormal.clone(),
                overflow,
                underflow: tiny.and(
                    controls
                        .underflow_masked()
                        .eq(false)
                        .or(controls.flush_to_zero())
                        .or(bounded.inexact),
                ),
                precision,
            },
        }
    }
}
