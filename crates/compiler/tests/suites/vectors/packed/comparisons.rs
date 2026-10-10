use super::*;

fn compare<T: VectorLane>(v8: bool, left: u128, right: u128, equal: u128, greater: u128) {
    binary_results(v8, left, right, [equal, greater], |left, right| {
        [left.eq_lanes::<T>(&right), left.signed_gt_lanes::<T>(right)]
    });
}

fn comparisons(v8: bool) {
    compare::<I8>(
        v8,
        0x4433ff80007ffe12_557f800100ff7f80,
        0x55220080017fff12_547f8001ff00807f,
        0x000000ff00ff00ff_00ffffff00000000,
        0x00ff000000000000_ff000000ff00ff00,
    );
    compare::<I16>(
        v8,
        0x00008000feff1234_0000ffff7fff8000,
        0xffff8000ff001234_0001ffff80007fff,
        0x0000ffff0000ffff_0000ffff00000000,
        0xffff000000000000_00000000ffff0000,
    );
    compare::<I32>(
        v8,
        0x00000000ffffffff_7fffffff80000000,
        0x00000001ffffffff_800000007fffffff,
        0x00000000ffffffff_0000000000000000,
        0x0000000000000000_ffffffff00000000,
    );
    compare::<I64>(
        v8,
        0xffffffffffffffff_8000000000000000,
        0xffffffffffffffff_7fffffffffffffff,
        0xffffffffffffffff_0000000000000000,
        0,
    );
    compare::<I64>(
        v8,
        0x0000000000000000_7fffffffffffffff,
        0xffffffffffffffff_8000000000000000,
        0,
        u128::MAX,
    );
}

#[test]
fn packed_comparison_masks_and_signed_extrema() {
    comparisons(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_packed_comparison_masks_and_signed_extrema() {
    comparisons(true);
}
