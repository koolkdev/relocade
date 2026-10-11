use super::*;

fn multiplication(v8: bool) {
    binary_results(
        v8,
        0x000080007fffffff7fff8000ffff1234,
        0xffffffffffff00027fff800000022345,
        [0x000080008001fffe00010000fffe0404],
        |left, right| [left.mul_lanes::<I16>(right)],
    );
    binary_results(
        v8,
        0x80000000ffffffff7fffffff00010000,
        0xffffffff000000028000000000010000,
        [0x80000000fffffffe8000000000000000],
        |left, right| [left.mul_lanes::<I32>(right)],
    );
    binary_results(
        v8,
        0x8000000000000000ffffffffffffffff,
        0xffffffffffffffff0000000000000002,
        [0x8000000000000000fffffffffffffffe],
        |left, right| [left.mul_lanes::<I64>(right)],
    );

    // Low and high widening select different halves, including signed boundary values.
    binary_results(
        v8,
        0x807f0100fffe827e7f80feff01020081,
        0x80ff7f0200feff017fff03fffe007f80,
        [
            0x3f010080fffa0001fffe000000003f80,
            0x4000ff81007f000000000004007e007e,
        ],
        |left, right| {
            [
                left.signed_widening_mul_low_lanes::<I8>(&right),
                left.signed_widening_mul_high_lanes::<I8>(right),
            ]
        },
    );
    binary_results(
        v8,
        0x80007fff0000fffe7fff8000ffff1234,
        0xffff0002000100037fff800000022345,
        [
            0x3fff000140000000fffffffe02820404,
            0x000080000000fffe00000000fffffffa,
        ],
        |left, right| {
            [
                left.signed_widening_mul_low_lanes::<I16>(&right),
                left.signed_widening_mul_high_lanes::<I16>(right),
            ]
        },
    );
    binary_results(
        v8,
        0xffffffff80000000800000007fffffff,
        0x7fffffff80000000ffffffff80000000,
        [
            0x0000000080000000c000000080000000,
            0xffffffff800000014000000000000000,
        ],
        |left, right| {
            [
                left.signed_widening_mul_low_lanes::<I32>(&right),
                left.signed_widening_mul_high_lanes::<I32>(right),
            ]
        },
    );

    // Low to high: all-minimum overflow, cancellation, negative and positive sums.
    binary_results(
        v8,
        0x7fff7fff80007fff0001ffff80008000,
        0x7fff7fff7fff80000001000180008000,
        [0x7ffe0002800100000000000080000000],
        |left, right| [left.signed_dot_i16_pairs(right)],
    );
}

#[test]
fn vector_products_preserve_signed_widths_and_pair_order() {
    multiplication(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_vector_products_preserve_signed_widths_and_pair_order() {
    multiplication(true);
}
