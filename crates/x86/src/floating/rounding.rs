//! Rounding preserves discarded-fraction evidence across shifts and formats.

use wasm86_compiler::{Val, F64, I1, I32, I64, I8};

#[cfg(test)]
mod tests;

/// An integer floor and its fractional remainder, described by the first
/// discarded bit and whether any lower bit is nonzero.
pub(crate) struct RoundingInput {
    pub(crate) integer: Val<I64>,
    pub(crate) guard: Val<I1>,
    pub(crate) sticky: Val<I1>,
}

impl RoundingInput {
    pub(crate) fn exact(integer: Val<I64>) -> Self {
        Self {
            integer,
            guard: false.into(),
            sticky: false.into(),
        }
    }

    /// Divides the unrounded magnitude by 2^distance, retaining earlier
    /// fractional evidence. Counts beyond a word do not wrap like Wasm shifts.
    pub(crate) fn shift_right(&self, distance: impl Into<Val<I32>>) -> Self {
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

pub(crate) struct Rounded {
    /// Wrapping integer result; consumers own exponent adjustment on carry.
    pub(crate) integer: Val<I64>,
    pub(crate) inexact: Val<I1>,
    pub(crate) incremented: Val<I1>,
}

pub(crate) struct RoundingMode(Val<I8>);

impl RoundingMode {
    pub(crate) fn new(control: Val<I8>) -> Self {
        Self(control.and(3))
    }

    /// Native integral rounding has the same RC policy and signed-zero rule.
    pub(crate) fn integral(&self, value: &Val<F64>) -> Val<F64> {
        self.0.eq(0).select(
            value.nearest(),
            self.0.eq(1).select(
                value.floor(),
                self.0.eq(2).select(value.ceil(), value.trunc()),
            ),
        )
    }

    /// Exact cancellation produces -0 only when rounding toward minus infinity.
    pub(crate) fn cancellation_negative(&self) -> Val<I1> {
        self.0.eq(1)
    }

    fn away_from_zero(&self, negative: &Val<I1>) -> Val<I1> {
        self.0
            .eq(1)
            .and(negative)
            .or(self.0.eq(2).and(negative.eq(false)))
    }

    pub(crate) fn overflow_to_infinity(&self, negative: &Val<I1>) -> Val<I1> {
        self.0.eq(0).or(self.away_from_zero(negative))
    }

    pub(crate) fn round(&self, input: RoundingInput, negative: &Val<I1>) -> Rounded {
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
