use super::*;

fn saturation() {
    binary_results(
        0x807fff00019c64c040ff0100817e807f,
        0x7f80807f00e41cc040fffeffff01ff01,
        [
            0xffff807f01807f807ffeffff807f807f,
            0xffffff7f01ff80ff80ffffffff7fff80,
        ],
        |left, right| {
            [
                left.signed_saturating_add_lanes::<I8>(&right),
                left.unsigned_saturating_add_lanes::<I8>(right),
            ]
        },
    );
    binary_results(
        0x807fff00019c64c040ff0100817e807f,
        0x7f80807f00e41cc040fffeffff01ff01,
        [
            0x807f7f8101b8480000000301827d817e,
            0x01007f000100480000000000007d007e,
        ],
        |left, right| {
            [
                left.signed_saturating_sub_lanes::<I8>(&right),
                left.unsigned_saturating_sub_lanes::<I8>(right),
            ]
        },
    );
    binary_results(
        0x4000ffff0001000080017ffe80007fff,
        0x4000fffffffefffffffe00020001ffff,
        [
            0x7ffffffeffffffff80007fff80017ffe,
            0x8000ffffffffffffffff80008001ffff,
        ],
        |left, right| {
            [
                left.signed_saturating_add_lanes::<I16>(&right),
                left.unsigned_saturating_add_lanes::<I16>(right),
            ]
        },
    );
    binary_results(
        0x4000ffff0001000080017ffe80007fff,
        0x4000fffffffefffffffe00020001ffff,
        [
            0x000000000003000180037ffc80007fff,
            0x000000000000000000007ffc7fff0000,
        ],
        |left, right| {
            [
                left.signed_saturating_sub_lanes::<I16>(&right),
                left.unsigned_saturating_sub_lanes::<I16>(right),
            ]
        },
    );
}

#[test]
fn packed_saturating_arithmetic_boundaries() {
    saturation();
}
