//! Exact 64-bit significand multiplication preserves the discarded product bits.

use super::{FiniteInput, FiniteMagnitude, RoundingInput};
use relocade_compiler::I32;

impl FiniteInput {
    /// Zero/special operand responses replace the numerical result and its evidence.
    pub(crate) fn multiply(&self, right: &Self) -> FiniteMagnitude {
        const LEADING: u64 = 1 << 63;
        let (low, high) = self.significand.unsigned().mul_wide(&right.significand);
        let upper_binade = high.and(LEADING).ne(0_u64);
        let integer = upper_binade.select(&high, high.shl(1).or(low.unsigned().shr(63)));
        let fraction = upper_binade.select(&low, low.shl(1));
        FiniteMagnitude {
            significand: RoundingInput {
                integer,
                guard: fraction.and(LEADING).ne(0_u64),
                sticky: fraction.and(LEADING - 1).ne(0_u64),
            },
            exponent: self
                .exponent
                .add(&right.exponent)
                .add(upper_binade.unsigned().extend::<I32>()),
            negative: self.negative.xor(&right.negative),
        }
    }
}
