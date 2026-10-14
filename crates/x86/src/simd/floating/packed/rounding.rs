//! Directed binary32 rounding adjusts a nearest result by at most one neighbor.

use super::*;

/// Masks are all-one dwords where nearest lies above/below the exact source.
/// Inactive lanes have zero masks. Callers own special-value and range admission.
pub(super) struct RoundedBinary32 {
    pub(super) nearest: Val<V128>,
    pub(super) above: Val<V128>,
    pub(super) below: Val<V128>,
}

impl RoundedBinary32 {
    pub(super) fn round(self, rounding: &RoundingMode) -> FloatResult<V128> {
        let negative = self.nearest.signed_shr_lanes::<I32>(31);
        let step_down = negative.select_bits(
            Val::<V128>::splat::<I32>(1_u32),
            Val::<V128>::splat::<I32>(u32::MAX),
        );
        let down = self.nearest.add_lanes::<I32>(self.above.and(&step_down));
        let up = self.nearest.sub_lanes::<I32>(self.below.and(step_down));
        let toward_zero = negative.select_bits(&up, &down);
        FloatResult {
            value: rounding.select(self.nearest, down, up, toward_zero),
            exceptions: SimdExceptions {
                precision: self.above.or(self.below).bitmask::<I32>().ne(0),
                ..SimdExceptions::default()
            },
        }
    }
}
