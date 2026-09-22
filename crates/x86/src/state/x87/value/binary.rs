//! Narrow real encodings expand exactly; operand exceptions remain separate.

use wasm86_compiler::{Val, I1, I16, I32, I64};

use super::ExtendedValue;

#[derive(Clone, Copy)]
pub(crate) enum BinaryFormat {
    Binary32,
    Binary64,
}

pub(crate) struct BinaryOperand {
    pub(crate) value: ExtendedValue,
    pub(crate) signaling_nan: Val<I1>,
    pub(crate) denormal: Val<I1>,
}

impl BinaryFormat {
    pub(crate) fn bytes(self) -> u32 {
        match self {
            Self::Binary32 => 4,
            Self::Binary64 => 8,
        }
    }

    /// The candidate quiets an SNaN, but its exception is resolved with the
    /// destination's stack fault before deciding whether to commit the load.
    pub(crate) fn decode(self, bits: &Val<I64>) -> BinaryOperand {
        let (fraction_bits, exponent_bits, bias) = match self {
            Self::Binary32 => (23, 8, 127),
            Self::Binary64 => (52, 11, 1023),
        };
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
        let nan = special.and(&nonzero_fraction);
        let fraction = fraction.shl(63 - fraction_bits);
        let signaling_nan = nan.and(fraction.and(1_u64 << 62).eq(0_u64));

        // Narrow subnormals are normal extended values. This exact expansion
        // uses neither the precision control nor the rounding control fields.
        let shift = fraction.clz();
        let significand = zero_exponent.select(
            fraction.shl(shift.truncate::<I32>()),
            fraction.or(1_u64 << 63).or(nan.select(1_u64 << 62, 0_u64)),
        );
        let finite_exponent = zero_exponent.select(
            nonzero_fraction.select(Val::<I64>::from((16384 - bias) as u64).sub(shift), 0_u64),
            exponent.add((16383 - bias) as u64),
        );
        let sign_exponent = sign.or(special
            .select(0x7fff_u64, finite_exponent)
            .truncate::<I16>());
        BinaryOperand {
            value: ExtendedValue {
                significand,
                sign_exponent,
            },
            signaling_nan,
            denormal: zero_exponent.and(nonzero_fraction),
        }
    }
}
