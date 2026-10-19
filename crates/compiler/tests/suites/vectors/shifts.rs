//! Shared vector shift counts are modulo the logical lane width.

use super::*;
use wasm86_compiler::{VectorLane, I16, I64};

fn cases<T: VectorLane>(width: u32, one: [u128; 3], last: [u128; 3]) {
    const INPUT: u128 = 0x80017fffffff0001_800000017fffffff;
    for (count, results) in [
        (0, [INPUT; 3]),
        (1, one),
        (width - 1, last),
        (width, [INPUT; 3]),
        (width + 1, one),
        (u32::MAX, last),
    ] {
        let mut bytes = vec![0xa5; 112];
        bytes[..16].copy_from_slice(&INPUT.to_le_bytes());
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &bytes);
        let module = fixture.function(&[Type::I32], &[], |mut body| {
            let runtime = body.load::<V128>(memory, 0)?;
            let dynamic_count = body.parameter::<I32>(0)?;
            let operations = |value: Val<V128>, count: Val<I32>| {
                [
                    value.shl_lanes::<T>(&count),
                    value.unsigned_shr_lanes::<T>(&count),
                    value.signed_shr_lanes::<T>(count),
                ]
            };
            for (index, result) in [
                operations(runtime, dynamic_count),
                operations(INPUT.into(), count.into()),
            ]
            .into_iter()
            .flatten()
            .enumerate()
            {
                body.store(memory, 16 + index as u32 * 16, result)?;
            }
            body.return_(())
        });
        let mut expected = bytes.clone();
        for (index, result) in [results, results].into_iter().flatten().enumerate() {
            expected[16 + index * 16..32 + index * 16].copy_from_slice(&result.to_le_bytes());
        }
        check(&module, &expected, count as i32);
    }
}

fn widths() {
    cases::<I8>(
        8,
        [
            0x0002fefefefe000200000002fefefefe,
            0x40003f7f7f7f0000400000003f7f7f7f,
            0xc0003fffffff0000c00000003fffffff,
        ],
        [
            0x00808080808000800000008080808080,
            0x01000001010100000100000000010101,
            0xff0000ffffff0000ff00000000ffffff,
        ],
    );
    cases::<I16>(
        16,
        [
            0x0002fffefffe000200000002fffefffe,
            0x40003fff7fff0000400000003fff7fff,
            0xc0003fffffff0000c00000003fffffff,
        ],
        [
            0x80008000800080000000800080008000,
            0x00010000000100000001000000000001,
            0xffff0000ffff0000ffff00000000ffff,
        ],
    );
    cases::<I32>(
        32,
        [
            0x0002fffefffe000200000002fffffffe,
            0x4000bfff7fff8000400000003fffffff,
            0xc000bfffffff8000c00000003fffffff,
        ],
        [
            0x80000000800000008000000080000000,
            0x00000001000000010000000100000000,
            0xffffffffffffffffffffffff00000000,
        ],
    );
    cases::<I64>(
        64,
        [
            0x0002fffffffe000200000002fffffffe,
            0x4000bfffffff800040000000bfffffff,
            0xc000bfffffff8000c0000000bfffffff,
        ],
        [
            0x80000000000000008000000000000000,
            0x00000000000000010000000000000001,
            0xffffffffffffffffffffffffffffffff,
        ],
    );
}

#[test]
fn packed_shifts_wrap_counts_at_lane_width() {
    widths();
    refined_count();
}

fn refined_count() {
    const INPUT: u128 = 0x0081_0081_0081_0081_0081_0081_0081_0081;
    let bytes = vec![0xa5; 40];
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let bits = Val::<V128>::from(INPUT);
        let byte_lanes = bits.shl_lanes::<I8>(&count);
        let word_lanes = bits.shl_lanes::<I16>(&count);
        let scalar = Val::<I8>::from(0x81).shl(&count);
        body.if_(count.eq(8), |mut arm| {
            arm.store(memory, 0, &byte_lanes)?;
            arm.store(memory, 16, &word_lanes)?;
            arm.store(memory, 32, &scalar)?;
            arm.return_(())
        })?;
        body.store(memory, 0, byte_lanes)?;
        body.store(memory, 16, word_lanes)?;
        body.store(memory, 32, scalar)?;
        body.return_(())
    });
    // A scalar byte masks the count modulo 32. Packed bytes and words retain
    // their own lane widths, including when the branch supplies a constant count.
    for (count, words, scalar) in [(8, INPUT << 8, 0), (32, INPUT, 0x81)] {
        let mut expected = bytes.clone();
        expected[..16].copy_from_slice(&INPUT.to_le_bytes());
        expected[16..32].copy_from_slice(&words.to_le_bytes());
        expected[32] = scalar;
        check(&module, &expected, count);
    }
}
