use super::*;

fn shuffle() {
    binary_results(
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
    shuffle();
}

fn runtime_indices() {
    let indices = [
        0, 15, 16, 31, 32, 33, 47, 48, 127, 128, 239, 240, 254, 255, 5, 20,
    ];
    let mut bytes = vec![0xa5; 96];
    bytes[..16].copy_from_slice(&LEFT.to_le_bytes());
    bytes[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    bytes[32..48].copy_from_slice(&indices);
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let left = body.load::<V128>(memory, 0)?;
        let right = body.load::<V128>(memory, 16)?;
        let runtime_indices = body.load::<V128>(memory, 32)?;
        body.store(memory, 48, left.shuffle_bytes(&right, runtime_indices))?;
        body.store(memory, 64, left.shuffle_bytes(&right, indices))?;
        body.store(
            memory,
            80,
            Val::<V128>::from(LEFT).shuffle_bytes(RIGHT, indices),
        )?;
        body.return_(())
    });
    let mut expected = bytes.clone();
    let result = [
        0x10, 0x01, 0x00, 0xf0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xba, 0xcc,
    ];
    for offset in [48, 64, 80] {
        expected[offset..offset + 16].copy_from_slice(&result);
    }
    check(&module, &expected, 0);
}

#[test]
fn vector_byte_shuffle_runtime_indices_and_zero_fill() {
    runtime_indices();
}

fn swizzle_union() {
    let even: [u8; 16] = std::array::from_fn(|i| if i % 2 == 0 { i as u8 } else { 255 });
    let odd: [u8; 16] = std::array::from_fn(|i| if i % 2 == 1 { i as u8 } else { 255 });
    let mut gap = odd;
    gap[15] = 255;
    binary_results(
        LEFT,
        RIGHT,
        [
            0xf0230f6755abaaef33dccc980054ff10,
            0x00f3006f00ab00ff00ff00dc00ff0010,
            0x00230f6755abaaef33dccc980054ff10,
        ],
        |left, right| {
            [
                left.swizzle_bytes(even).or(right.swizzle_bytes(odd)),
                // Both swizzles contribute bits to the same bytes.
                left.swizzle_bytes(even).or(right.swizzle_bytes(even)),
                // The last byte must stay zero when neither input supplies it.
                left.swizzle_bytes(even).or(right.swizzle_bytes(gap)),
            ]
        },
    );
}

#[test]
fn vector_swizzle_union_preserves_overlapping_bits_and_zero_fill() {
    swizzle_union();
}
