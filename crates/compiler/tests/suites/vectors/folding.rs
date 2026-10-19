//! Lane routing preserves scalar normalization and every observed vector byte.

use super::*;
use wasm86_compiler::I16;

fn routing() {
    let mut initial = vec![0xa5; 192];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    initial[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let x = body.load::<V128>(memory, 0)?;
        let y = body.load::<V128>(memory, 16)?;
        let input = body.parameter::<I32>(0)?;
        let dirty = input.truncate::<I16>().add(65535);
        body.store(
            memory,
            32,
            x.replace_lane(5, dirty)
                .extract_lane::<I16>(5)
                .unsigned()
                .extend::<I32>(),
        )?;
        body.store(
            memory,
            36,
            x.replace_lane(3, &input)
                .extract_lane::<I16>(0)
                .unsigned()
                .extend::<I32>(),
        )?;
        body.store(
            memory,
            40,
            x.replace_lane(0, &input)
                .extract_lane::<I16>(1)
                .unsigned()
                .extend::<I32>(),
        )?;
        body.store(memory, 48, x.replace_lane(5, x.extract_lane::<I16>(5)))?;
        body.store(
            memory,
            64,
            x.replace_lane(3, &input).replace_lane::<I32>(3, 0x12345678),
        )?;
        let reversed = x.shuffle_bytes(&x, std::array::from_fn(|i| 15 - i as u8));
        body.store(
            memory,
            80,
            reversed.shuffle_bytes(&reversed, std::array::from_fn(|i| 15 - i as u8)),
        )?;
        body.store(
            memory,
            96,
            x.shuffle_bytes(&y, std::array::from_fn(|i| 16 + i as u8)),
        )?;
        let low = x.low_half();
        body.store(
            memory,
            112,
            low.shuffle_bytes(y.low_half(), [0, 1, 2, 3, 4, 5, 6, 7])
                .zero_extend(),
        )?;
        let mixed = x.shuffle_bytes(
            &y,
            [0, 1, 2, 3, 16, 17, 18, 19, 8, 9, 10, 11, 24, 25, 26, 27],
        );
        let swapped = mixed.shuffle_bytes(
            &mixed,
            [4, 5, 6, 7, 0, 1, 2, 3, 12, 13, 14, 15, 8, 9, 10, 11],
        );
        body.store(memory, 128, &swapped)?;
        body.store(memory, 144, swapped.extract_lane::<I32>(0))?;
        // This routing needs x, y and the literal; one two-input shuffle cannot
        // replace it. Its first scalar lane also gathers noncontiguous bytes.
        let gather = mixed.shuffle_bytes(
            [0x42_u8; 16],
            [0, 4, 16, 1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14],
        );
        body.store(memory, 160, &gather)?;
        body.store(memory, 176, gather.extract_lane::<I32>(0))?;
        body.return_(())
    });
    let mut expected = initial.clone();
    expected[36..40].copy_from_slice(&0x3210_u32.to_le_bytes());
    expected[48..64].copy_from_slice(&LEFT.to_le_bytes());
    expected[64..80].copy_from_slice(&LEFT.to_le_bytes());
    expected[76..80].copy_from_slice(&0x12345678_u32.to_le_bytes());
    expected[80..96].copy_from_slice(&LEFT.to_le_bytes());
    expected[96..112].copy_from_slice(&RIGHT.to_le_bytes());
    expected[112..128].copy_from_slice(&u128::from(LEFT as u64).to_le_bytes());
    let a = LEFT.to_le_bytes();
    let b = RIGHT.to_le_bytes();
    expected[128..132].copy_from_slice(&b[..4]);
    expected[132..136].copy_from_slice(&a[..4]);
    expected[136..140].copy_from_slice(&b[8..12]);
    expected[140..144].copy_from_slice(&a[8..12]);
    expected[144..148].copy_from_slice(&b[..4]);
    expected[160..176].copy_from_slice(&[
        a[0], b[0], 0x42, a[1], a[2], a[3], b[1], b[2], b[3], a[8], a[9], a[10], a[11], b[8], b[9],
        b[10],
    ]);
    expected[176..180].copy_from_slice(&[a[0], b[0], 0x42, a[1]]);
    for input in [0_u32, 7, 65535, 0x1234ffff, u32::MAX] {
        expected[32..36].copy_from_slice(&u32::from((input as u16).wrapping_sub(1)).to_le_bytes());
        expected[40..44].copy_from_slice(&(input >> 16).to_le_bytes());
        check(&module, &expected, input as i32);
    }
    let mut shuffles = 0;
    for payload in wasmparser::Parser::new(0).parse_all(module.bytes()) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                if matches!(op.unwrap(), wasmparser::Operator::I8x16Shuffle { .. }) {
                    shuffles += 1;
                }
            }
        }
    }
    assert_eq!(
        shuffles, 3,
        "one composed shuffle and two for the three-source gather"
    );
}

#[test]
fn lane_and_shuffle_folding_preserves_routed_bytes_and_narrow_scalars() {
    routing();
}
