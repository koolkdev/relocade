//! Directed floating-point rounding adjusts a nearest result by at most one neighbor.

use super::*;

/// Masks are all-one lanes where nearest lies above/below the exact source.
/// Inactive lanes have zero masks. Callers own special-value and range admission.
pub(super) struct RoundedFloat {
    pub(super) format: BinaryFormat,
    pub(super) nearest: Val<V128>,
    pub(super) above: Val<V128>,
    pub(super) below: Val<V128>,
}

impl RoundedFloat {
    pub(super) fn round(self, rounding: &RoundingMode) -> FloatResult<V128> {
        macro_rules! neighbors {
            ($lane:ty, $scalar:ty, $sign:literal) => {{
                let negative = self.nearest.signed_shr_lanes::<$lane>($sign);
                let step_down = negative.select_bits(
                    Val::<V128>::splat::<$lane>(1 as $scalar),
                    Val::<V128>::splat::<$lane>(<$scalar>::MAX),
                );
                let down = self.nearest.add_lanes::<$lane>(self.above.and(&step_down));
                let up = self.nearest.sub_lanes::<$lane>(self.below.and(step_down));
                let toward_zero = negative.select_bits(&up, &down);
                rounding.select(self.nearest, down, up, toward_zero)
            }};
        }
        let value = match self.format {
            BinaryFormat::Binary32 => neighbors!(I32, u32, 31),
            BinaryFormat::Binary64 => neighbors!(I64, u64, 63),
        };
        FloatResult {
            value,
            exceptions: SimdExceptions {
                precision: self.above.or(self.below).bitmask::<I32>().ne(0),
                ..SimdExceptions::default()
            },
        }
    }
}
