//! Integer rounding retains the discarded fraction independently of its result.

use wasm86_compiler::{Val, I1, I32, I64, I8};

#[cfg(test)]
mod tests;

/// An integer floor and its fractional remainder, described by the first
/// discarded bit and whether any lower bit is nonzero.
pub(super) struct RoundingInput {
    pub(super) integer: Val<I64>,
    pub(super) guard: Val<I1>,
    pub(super) sticky: Val<I1>,
}

impl RoundingInput {
    pub(super) fn exact(integer: Val<I64>) -> Self {
        Self {
            integer,
            guard: false.into(),
            sticky: false.into(),
        }
    }

    /// Divides the unrounded magnitude by 2^distance, retaining earlier
    /// fractional evidence. Counts beyond a word do not wrap like Wasm shifts.
    pub(super) fn shift_right(&self, distance: impl Into<Val<I32>>) -> Self {
        let distance = distance.into();
        let value = &self.integer;
        let shifted = distance.ne(0);
        let below_word = distance.unsigned().lt(64);
        let integer = below_word.select(value.unsigned().shr(&distance), 0_u64);
        let guard = shifted
            .and(distance.unsigned().lt(65))
            .and(value.unsigned().shr(distance.sub(1)).and(1_u64).ne(0_u64))
            .or(shifted.eq(false).and(&self.guard));
        let lower_mask = Val::<I64>::from(1_u64).shl(distance.sub(1)).sub(1_u64);
        let sticky = self.sticky.or(shifted.and(
            self.guard.or(distance
                .unsigned()
                .ge(65)
                .select(value.ne(0_u64), value.and(lower_mask).ne(0_u64))),
        ));
        Self {
            integer,
            guard,
            sticky,
        }
    }
}

pub(super) struct Rounded {
    /// Wrapping integer result; consumers own exponent adjustment on carry.
    pub(super) integer: Val<I64>,
    pub(super) inexact: Val<I1>,
    pub(super) incremented: Val<I1>,
}

pub(crate) struct RoundingMode(Val<I8>);

impl RoundingMode {
    pub(crate) fn new(control: Val<I8>) -> Self {
        Self(control.and(3))
    }

    fn away_from_zero(&self, negative: &Val<I1>) -> Val<I1> {
        self.0
            .eq(1)
            .and(negative)
            .or(self.0.eq(2).and(negative.eq(false)))
    }

    pub(super) fn overflow_to_infinity(&self, negative: &Val<I1>) -> Val<I1> {
        self.0.eq(0).or(self.away_from_zero(negative))
    }

    pub(super) fn round(&self, input: RoundingInput, negative: &Val<I1>) -> Rounded {
        let inexact = input.guard.or(&input.sticky);
        let nearest = input
            .guard
            .and(input.sticky.or(input.integer.and(1_u64).ne(0_u64)));
        let incremented = self
            .0
            .eq(0)
            .select(nearest, inexact.and(self.away_from_zero(negative)));
        Rounded {
            integer: input.integer.add(incremented.unsigned().extend::<I64>()),
            inexact,
            incremented,
        }
    }
}
