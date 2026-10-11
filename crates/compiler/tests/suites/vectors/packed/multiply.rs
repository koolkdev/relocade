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

    // Widening consumes only the low half, including signed boundary values.
    binary_results(
        v8,
        0xdeadbeef123456787f80feff01020081,
        0x76543210feedface7fff03fffe007f80,
        [0x3f010080fffa0001fffe000000003f80],
        |left, right| [left.signed_widening_mul_low_lanes::<I8>(right)],
    );
    binary_results(
        v8,
        0xdeadbeef123456787fff8000ffff1234,
        0x76543210feedface7fff800000022345,
        [0x3fff000140000000fffffffe02820404],
        |left, right| [left.signed_widening_mul_low_lanes::<I16>(right)],
    );
    binary_results(
        v8,
        0xdeadbeef12345678800000007fffffff,
        0x76543210feedfaceffffffff80000000,
        [0x0000000080000000c000000080000000],
        |left, right| [left.signed_widening_mul_low_lanes::<I32>(right)],
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
