use super::*;

fn narrowing() {
    binary_results(
        0x7fff0000ffff80000080007fff80ff7f,
        0xff7eff81007e00010000ffff010000ff,
        [
            0x80817e0100ff7f7f7f00ff807f7f8080,
            0x00007e010000ffffff000000807f0000,
        ],
        |left, right| {
            [
                left.signed_narrow_lanes::<I8>(&right),
                left.unsigned_narrow_lanes::<I8>(right),
            ]
        },
    );
    binary_results(
        0x0000800000007fffffff8000ffff7fff,
        0x00010000ffffffff7fffffff80000000,
        [
            0x7fffffff7fff80007fff7fff80008000,
            0xffff0000ffff000080007fff00000000,
        ],
        |left, right| {
            [
                left.signed_narrow_lanes::<I16>(&right),
                left.unsigned_narrow_lanes::<I16>(right),
            ]
        },
    );
}

#[test]
fn vector_narrowing_saturates_signed_inputs_and_preserves_order() {
    narrowing();
}
