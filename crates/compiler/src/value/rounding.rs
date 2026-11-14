use super::{construction, Val};
use crate::{expression::rounding, Expression, I1, I32, I64, I8};

/// An unsigned integer floor and the fractional evidence below it.
pub struct RoundingInput {
    pub integer: Val<I64>,
    /// The first bit after the binary point.
    pub guard: Val<I1>,
    /// Whether any bit below the guard bit is nonzero.
    pub sticky: Val<I1>,
}

pub struct RoundedInteger {
    /// Wrapping integer result; the caller owns carry and exponent adjustment.
    pub integer: Val<I64>,
    pub inexact: Val<I1>,
    pub incremented: Val<I1>,
}

impl RoundingInput {
    pub fn exact(integer: impl Into<Val<I64>>) -> Self {
        Self {
            integer: integer.into(),
            guard: false.into(),
            sticky: false.into(),
        }
    }

    /// Divide by 2^distance, then round the magnitude. Counts beyond a word do
    /// not wrap. Nearest-even takes precedence; otherwise `away_from_zero`
    /// chooses between increasing the magnitude and truncating it.
    pub fn round(
        &self,
        distance: impl Into<Val<I32>>,
        nearest_even: impl Into<Val<I1>>,
        away_from_zero: impl Into<Val<I1>>,
    ) -> RoundedInteger {
        let flag =
            |value: &Val<I1>, mask: u8| value.unsigned().extend::<I8>().shl(mask.trailing_zeros());
        let control = flag(&self.guard, rounding::GUARD)
            .or(flag(&self.sticky, rounding::STICKY))
            .or(flag(&nearest_even.into(), rounding::NEAREST))
            .or(flag(&away_from_zero.into(), rounding::AWAY));
        let (integer, inexact, incremented) =
            construction::expression_results::<(I64, I1, I1)>(Expression::RoundMagnitude {
                integer: (&self.integer).into(),
                distance: distance.into().into(),
                control: control.into(),
            });
        RoundedInteger {
            integer,
            inexact,
            incremented,
        }
    }

    /// Divide an unrounded magnitude while retaining its fractional evidence.
    pub fn shift_right(&self, distance: impl Into<Val<I32>>) -> Self {
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
