//! Byte differences are accumulated independently in each eight-byte group.

use relocade_compiler::{Val, VectorType, I16, I64, I8};

pub(crate) fn sum_absolute_byte_differences<V: VectorType>(left: Val<V>, right: Val<V>) -> Val<V> {
    let differences = left
        .unsigned_max_lanes::<I8>(&right)
        .sub_lanes::<I8>(left.unsigned_min_lanes::<I8>(right));
    let sums = differences
        .unsigned_pairwise_add_lanes::<I8>()
        .unsigned_pairwise_add_lanes::<I16>();
    // Join the four-byte sums; each total is at most 8 * 255 and occupies only
    // the low word of its qword. All other destination bits are cleared.
    sums.add_lanes::<I64>(sums.unsigned_shr_lanes::<I64>(32))
        .and(Val::<V>::splat::<I64>(0xffff_u64))
}
