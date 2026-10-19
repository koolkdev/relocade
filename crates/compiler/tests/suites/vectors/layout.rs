//! SIMD primitives agree for computed operands and literal folding.

use super::*;
use relocade_compiler::{I16, I64};

#[test]
fn vector_swizzle_zeroes_every_out_of_range_byte_index() {
    let input: [u8; 16] = std::array::from_fn(|i| 0x80 + i as u8);
    let mut cases: Vec<([u8; 16], [u8; 16])> = (0..16)
        .map(|group| {
            (
                std::array::from_fn(|i| group * 16 + i as u8),
                if group == 0 { input } else { [0; 16] },
            )
        })
        .collect();
    cases.push((
        [15, 0, 16, 255, 7, 7, 8, 32, 128, 1, 14, 254, 3, 240, 2, 13],
        [
            0x8f, 0x80, 0, 0, 0x87, 0x87, 0x88, 0, 0, 0x81, 0x8e, 0, 0x83, 0, 0x82, 0x8d,
        ],
    ));
    for (indices, result) in cases {
        let mut bytes = vec![0xa5; 80];
        bytes[..16].copy_from_slice(&input);
        bytes[16..32].copy_from_slice(&indices);
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &bytes);
        let module = fixture.function(&[Type::I32], &[], |mut body| {
            let vector = body.load::<V128>(memory, 0)?;
            let runtime_indices = body.load::<V128>(memory, 16)?;
            body.store(memory, 32, vector.swizzle_bytes(runtime_indices))?;
            body.store(memory, 48, vector.swizzle_bytes(indices))?;
            body.store(memory, 64, Val::<V128>::from(input).swizzle_bytes(indices))?;
            body.return_(())
        });
        let mut expected = bytes.clone();
        for offset in [32, 48, 64] {
            expected[offset..offset + 16].copy_from_slice(&result);
        }
        check(&module, &expected, 0);
    }
    let indices = [7, 8, 15, 16, 255, 0, 6, 128];
    let mut bytes = vec![0xa5; 48];
    bytes[..16].copy_from_slice(&input);
    bytes[16..24].copy_from_slice(&indices);
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let vector = body.load::<V128>(memory, 0)?.low_half();
        let runtime_indices = body.load::<V128>(memory, 16)?.low_half();
        body.store(memory, 24, vector.swizzle_bytes(runtime_indices))?;
        body.store(memory, 32, vector.swizzle_bytes(indices))?;
        body.store(
            memory,
            40,
            Val::<relocade_compiler::V64>::from(0x8786858483828180_u64).swizzle_bytes(indices),
        )?;
        body.return_(())
    });
    let mut expected = bytes.clone();
    for output in [24, 32, 40] {
        expected[output..output + 8].copy_from_slice(&[0x87, 0, 0, 0, 0, 0x80, 0x86, 0]);
    }
    check(&module, &expected, 0);
}

#[test]
fn vector_selection_uses_individual_mask_bits() {
    let mask = 0x00ff_aa55_f00f_0180_ffff_0000_5555_aaaa_u128;
    let mut bytes = vec![0xa5; 160];
    for (offset, value) in [(0, LEFT), (16, RIGHT), (32, mask)] {
        bytes[offset..offset + 16].copy_from_slice(&value.to_le_bytes());
    }
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let left = body.load::<V128>(memory, 0)?;
        let right = body.load::<V128>(memory, 16)?;
        let runtime_mask = body.load::<V128>(memory, 32)?;
        let results = [
            runtime_mask.select_bits(&left, &right),
            Val::<V128>::from(mask).select_bits(&left, &right),
            Val::<V128>::from(mask).select_bits(LEFT, RIGHT),
            Val::<V128>::from(0_u128).select_bits(&left, &right),
            Val::<V128>::from(u128::MAX).select_bits(&left, &right),
            runtime_mask.select_bits(&left, &left),
        ];
        for (i, result) in results.into_iter().enumerate() {
            body.store(memory, 64 + i as u32 * 16, result)?;
        }
        body.return_(())
    });
    let mut expected = bytes.clone();
    let result = (LEFT & mask) | (RIGHT & !mask);
    for (i, value) in [result, result, result, RIGHT, LEFT, LEFT]
        .into_iter()
        .enumerate()
    {
        expected[64 + i * 16..80 + i * 16].copy_from_slice(&value.to_le_bytes());
    }
    check(&module, &expected, 0);
}

#[test]
fn vector_splat_preserves_logical_lane_widths() {
    let bytes = vec![0xa5; 128];
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let wide = input.unsigned().extend::<I64>().shl(32).or(0x98765432_u64);
        let results = [
            Val::<V128>::splat(input.truncate::<I8>().add(7)),
            Val::<V128>::splat(input.truncate::<I16>().add(7)),
            Val::<V128>::splat(input.add(7)),
            Val::<V128>::splat(&wide),
            Val::<V128>::splat(Val::<I8>::from(0xfd).add(7)),
            Val::<V128>::splat(Val::<I16>::from(0xfffd).add(7)),
            Val::<V128>::splat(Val::<I32>::from(0x1234fffd).add(7)),
            Val::<V128>::splat::<I64>(0x1234fffd98765432_u64),
        ];
        for (i, result) in results.into_iter().enumerate() {
            body.store(memory, i as u32 * 16, result)?;
        }
        body.return_(())
    });
    let results = [
        0x04040404_04040404_04040404_04040404_u128,
        0x00040004_00040004_00040004_00040004,
        0x12350004_12350004_12350004_12350004,
        0x1234fffd98765432_1234fffd98765432,
    ];
    let expected: Vec<u8> = results
        .into_iter()
        .cycle()
        .take(8)
        .flat_map(u128::to_le_bytes)
        .collect();
    check(&module, &expected, 0x1234fffd);
}
