use super::*;

fn shuffle(v8: bool) {
    binary_results(
        v8,
        LEFT,
        RIGHT,
        [
            0x33dcaacd23f0ff3233fe55ef010010f0,
            0xf0f00f0f012345673333ccccfedcba98,
        ],
        |left, right| {
            [
                left.shuffle_bytes(
                    &right,
                    [31, 0, 16, 15, 8, 24, 7, 23, 1, 17, 30, 14, 9, 25, 6, 22],
                ),
                left.shuffle_bytes(
                    &right,
                    [4, 5, 6, 7, 20, 21, 22, 23, 12, 13, 14, 15, 28, 29, 30, 31],
                ),
            ]
        },
    );
}

#[test]
fn vector_byte_shuffle_selects_both_inputs() {
    shuffle(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_vector_byte_shuffle_selects_both_inputs() {
    shuffle(true);
}

#[test]
#[should_panic(expected = "shuffle byte is in range")]
fn vector_byte_shuffle_rejects_out_of_range_index() {
    Val::<V128>::from(0_u128).shuffle_bytes(0_u128, [32; 16]);
}
