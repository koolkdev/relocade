//! Rounding preserves discarded-fraction evidence across shifts and formats.

use relocade_compiler::{Val, ValueType, F64, I1, I32, I8};

#[cfg(test)]
mod tests;

pub(crate) use relocade_compiler::{RoundedInteger as Rounded, RoundingInput};

pub(crate) struct RoundingMode(Val<I8>);

impl RoundingMode {
    pub(crate) fn new(control: Val<I8>) -> Self {
        Self(control.and(3))
    }

    pub(crate) fn is_nearest(&self) -> Val<I1> {
        self.0.eq(0)
    }

    /// Native integral rounding has the same RC policy and signed-zero rule.
    pub(crate) fn integral(&self, value: &Val<F64>) -> Val<F64> {
        self.select(value.nearest(), value.floor(), value.ceil(), value.trunc())
    }

    /// Choose results already rounded in the four architectural directions.
    pub(crate) fn select<T: ValueType>(
        &self,
        nearest: Val<T>,
        down: Val<T>,
        up: Val<T>,
        zero: Val<T>,
    ) -> Val<T> {
        self.is_nearest().select(
            nearest,
            self.0.eq(1).select(down, self.0.eq(2).select(up, zero)),
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

    pub(crate) fn round(
        &self,
        input: &RoundingInput,
        distance: impl Into<Val<I32>>,
        negative: &Val<I1>,
    ) -> Rounded {
        input.round(distance, self.is_nearest(), self.away_from_zero(negative))
    }
}
