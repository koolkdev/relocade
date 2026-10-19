//! Packed integer operations agree for constants and runtime inputs.

#[path = "packed/average.rs"]
mod average;
#[path = "packed/comparisons.rs"]
mod comparisons;
#[path = "packed/identities.rs"]
mod identities;
#[path = "packed/minmax.rs"]
mod minmax;
#[path = "packed/multiply.rs"]
mod multiply;
#[path = "packed/narrow.rs"]
mod narrow;
#[path = "packed/pairwise.rs"]
mod pairwise;
#[path = "packed/saturation.rs"]
mod saturation;
#[path = "packed/shuffle.rs"]
mod shuffle;

use super::*;
use wasm86_compiler::{MemoryType, VectorLane, VectorType, I16, I64, V64};

fn arithmetic<T: VectorLane>(left: u128, right: u128, sum: u128, difference: u128) {
    binary_results(left, right, [sum, difference], |left, right| {
        [left.add_lanes::<T>(&right), left.sub_lanes::<T>(right)]
    });
}

fn binary_results<const N: usize>(
    left: u128,
    right: u128,
    results: [u128; N],
    operations: impl Fn(Val<V128>, Val<V128>) -> [Val<V128>; N],
) {
    binary_width_results::<V128, N>(left, right, results, operations);
}

fn binary_width_results<V: VectorType + MemoryType, const N: usize>(
    left: u128,
    right: u128,
    results: [u128; N],
    operations: impl Fn(Val<V>, Val<V>) -> [Val<V>; N],
) {
    let width = V::BYTES as usize;
    let literal = |bits: u128| {
        let mut bytes = V::Bytes::default();
        bytes.as_mut().copy_from_slice(&bits.to_le_bytes()[..width]);
        bytes.into()
    };
    let mut bytes = vec![0xa5; 32 + 2 * width * N];
    bytes[..16].copy_from_slice(&left.to_le_bytes());
    bytes[16..32].copy_from_slice(&right.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let runtime_left = body.load::<V>(memory, 0)?;
        let runtime_right = body.load::<V>(memory, 16)?;
        let constant_left = literal(left);
        let results = [
            operations(runtime_left, runtime_right),
            operations(constant_left, literal(right)),
        ];
        for (index, result) in results.into_iter().flatten().enumerate() {
            body.store(memory, 32 + index as u32 * V::BYTES, result)?;
        }
        body.return_(())
    });
    let mut expected = bytes.clone();
    for (index, result) in [results, results].into_iter().flatten().enumerate() {
        expected[32 + index * width..32 + (index + 1) * width]
            .copy_from_slice(&result.to_le_bytes()[..width]);
    }
    check(&module, &expected, 0);
}

fn arithmetic_widths() {
    arithmetic::<I8>(
        0xf0debc9a78563412_ff01fe1000807fff,
        0x1123ff12087f80ff_ffff03f001ff0101,
        0x0101bbac80d5b411_fe000100017f8000,
        0xdfbbbd8870d7b413_0002fb20ff817efe,
    );
    arithmetic::<I16>(
        0x0001ff00abcd1234_000080007fffffff,
        0xffff01ff5432fedc_0001ffff00010001,
        0x000000ffffff1110_00017fff80000000,
        0x0002fd01579b1358_ffff80017ffefffe,
    );
    arithmetic::<I32>(
        0x0000000080000000_7fffffffffffffff,
        0x00000001ffffffff_0000000100000001,
        0x000000017fffffff_8000000000000000,
        0xffffffff80000001_7ffffffefffffffe,
    );
    arithmetic::<I64>(
        0x8000000000000000_ffffffffffffffff,
        0xffffffffffffffff_0000000000000001,
        0x7fffffffffffffff_0000000000000000,
        0x8000000000000001_fffffffffffffffe,
    );
}

fn narrow_lanes() {
    const VECTOR: u128 = 0xffeeddcc_bbaa9988_77665544_33221180;
    let mut bytes = vec![0xa5; 96];
    bytes[..16].copy_from_slice(&VECTOR.to_le_bytes());
    bytes[16..20].copy_from_slice(&255_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&65535_u32.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let vector = body.load::<V128>(memory, 0)?;
        let byte = body.load::<I32>(memory, 16)?.truncate::<I8>().add(3);
        let word = body.load::<I32>(memory, 20)?.truncate::<I16>().add(2);
        body.store(
            memory,
            32,
            vector.replace_lane(15, byte).replace_lane(0, word),
        )?;
        let constant = Val::<V128>::from(VECTOR)
            .replace_lane(15, Val::<I8>::from(255).add(3))
            .replace_lane(0, Val::<I16>::from(65535).add(2));
        body.store(memory, 48, constant)?;
        body.store(
            memory,
            64,
            vector.extract_lane::<I8>(15).unsigned().extend::<I32>(),
        )?;
        body.store(
            memory,
            68,
            vector.extract_lane::<I16>(7).unsigned().extend::<I32>(),
        )?;
        body.store(
            memory,
            72,
            Val::<V128>::from(VECTOR)
                .extract_lane::<I8>(15)
                .unsigned()
                .extend::<I32>(),
        )?;
        body.store(
            memory,
            76,
            Val::<V128>::from(VECTOR)
                .extract_lane::<I16>(7)
                .unsigned()
                .extend::<I32>(),
        )?;
        body.return_(())
    });
    let mut expected = bytes.clone();
    let replaced = 0x02eeddcc_bbaa9988_77665544_33220001_u128.to_le_bytes();
    expected[32..48].copy_from_slice(&replaced);
    expected[48..64].copy_from_slice(&replaced);
    expected[64..80].copy_from_slice(&[
        0xff, 0, 0, 0, 0xee, 0xff, 0, 0, 0xff, 0, 0, 0, 0xee, 0xff, 0, 0,
    ]);
    check(&module, &expected, 0);
}

#[test]
fn packed_arithmetic_and_narrow_lane_views() {
    arithmetic_widths();
    narrow_lanes();
}
