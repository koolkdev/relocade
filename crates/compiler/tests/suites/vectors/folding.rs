//! Lane folds preserve narrow carriers and every independently observed bit.

use super::*;
use wasm86_compiler::{VectorLane, I16, I64};

fn identities<T: VectorLane>(v8: bool) {
    let mut initial = vec![0xa5; 112];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let input = body.load::<V128>(memory, 0)?;
        let results = [
            input.sub_lanes::<T>(&input),
            input.eq_lanes::<T>(&input),
            input.signed_gt_lanes::<T>(&input),
            input.add_lanes::<T>(0_u128),
            Val::<V128>::from(0_u128).add_lanes::<T>(&input),
            input.sub_lanes::<T>(0_u128),
        ];
        for (index, result) in results.into_iter().enumerate() {
            body.store(memory, 16 + index as u32 * 16, result)?;
        }
        body.return_(())
    });
    assert!(!operators(&module).iter().any(|op| matches!(
        op,
        Operator::I8x16Add
            | Operator::I16x8Add
            | Operator::I32x4Add
            | Operator::I64x2Add
            | Operator::I8x16Sub
            | Operator::I16x8Sub
            | Operator::I32x4Sub
            | Operator::I64x2Sub
            | Operator::I8x16Eq
            | Operator::I16x8Eq
            | Operator::I32x4Eq
            | Operator::I64x2Eq
            | Operator::I8x16GtS
            | Operator::I16x8GtS
            | Operator::I32x4GtS
            | Operator::I64x2GtS
    )));
    let mut expected = initial.clone();
    for (index, value) in [0, u128::MAX, 0, LEFT, LEFT, LEFT].into_iter().enumerate() {
        expected[16 + index * 16..32 + index * 16].copy_from_slice(&value.to_le_bytes());
    }
    check(&module, &initial, &expected, 0, v8);
}

fn narrow_carriers(v8: bool) {
    let mut initial = vec![0xa5; 32];
    initial[..4].copy_from_slice(&255_u32.to_le_bytes());
    initial[4..8].copy_from_slice(&65535_u32.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let byte = body.load::<I32>(memory, 0)?.truncate::<I8>();
        let word = body.load::<I32>(memory, 4)?.truncate::<I16>();
        let base = Val::<V128>::from(LEFT);
        let results = [
            base.replace_lane(15, byte.add(3))
                .extract_lane::<I8>(15)
                .unsigned()
                .extend::<I32>(),
            base.replace_lane(7, word.add(2))
                .extract_lane::<I16>(7)
                .unsigned()
                .extend::<I32>(),
            base.replace_lane(0, byte.signed().extend::<I32>().truncate::<I8>())
                .extract_lane::<I8>(0)
                .unsigned()
                .extend::<I32>(),
            base.replace_lane(0, word.signed().extend::<I32>().truncate::<I16>())
                .extract_lane::<I16>(0)
                .unsigned()
                .extend::<I32>(),
        ];
        for (index, value) in results.into_iter().enumerate() {
            body.store(memory, 16 + index as u32 * 4, value)?;
        }
        body.return_(())
    });
    assert!(!operators(&module).iter().any(|op| matches!(
        op,
        Operator::I8x16ReplaceLane { .. }
            | Operator::I16x8ReplaceLane { .. }
            | Operator::I8x16ExtractLaneU { .. }
            | Operator::I16x8ExtractLaneU { .. }
    )));
    let mut expected = initial.clone();
    for (index, value) in [2_u32, 1, 255, 65535].into_iter().enumerate() {
        expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    check(&module, &initial, &expected, 0, v8);
}

fn shared_vector(v8: bool, scalar_first: bool) {
    let mut initial = vec![0xa5; 64];
    initial[..16].copy_from_slice(&0x0001_0002_0003_0004_0005_0006_0007_0008_u128.to_le_bytes());
    initial[16..32].copy_from_slice(&0x0001_0002_0003_0004_0005_9999_0007_aaaa_u128.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let left = body.load::<V128>(memory, 0)?;
        let right = body.load::<V128>(memory, 16)?;
        let compared = left.eq_lanes::<I16>(&right);
        let half = Val::<V128>::from(0_u128).replace_lane(0, compared.extract_lane::<I64>(0));
        let sum = half.add_lanes::<I16>(&right);
        let scalar = sum.extract_lane::<I64>(0);
        // The full sum needs zero upper input lanes. Retain its shared producer
        // even when the scalar extraction is stored first.
        if scalar_first {
            body.store(memory, 48, scalar)?;
            body.store(memory, 32, &sum)?;
        } else {
            body.store(memory, 32, &sum)?;
            body.store(memory, 48, scalar)?;
        }
        body.store(memory, 56, compared.extract_lane::<I64>(1))?;
        body.return_(())
    });
    assert_eq!(
        operators(&module)
            .iter()
            .filter(|op| matches!(op, Operator::I16x8Add))
            .count(),
        1
    );
    let mut expected = initial.clone();
    expected[32..48].copy_from_slice(&0x0001_0002_0003_0004_0004_9999_0006_aaaa_u128.to_le_bytes());
    expected[48..56].copy_from_slice(&0x0004_9999_0006_aaaa_u64.to_le_bytes());
    expected[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
    check(&module, &initial, &expected, 0, v8);
}

fn lane_dependencies(v8: bool) {
    let mut initial = vec![0xa5; 80];
    initial[..16].copy_from_slice(&0x1234_5678_90ab_cdef_0100_0000_0000_00ff_u128.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let input = body.load::<V128>(memory, 0)?;
        let byte = Val::<V128>::from(0_u128).replace_lane(0, input.extract_lane::<I8>(0));
        let high = Val::<V128>::from(0_u128).replace_lane(1, input.extract_lane::<I64>(1));
        let moved = Val::<V128>::from(0_u128).replace_lane(0, input.extract_lane::<I64>(1));
        let overlapping = input.replace_lane::<I8>(1, 0xab);
        let results = [
            // Reading one comparison byte still needs the other 56 input bits.
            byte.eq_lanes::<I64>(0xff_u128)
                .extract_lane::<I8>(0)
                .unsigned()
                .extend::<I32>(),
            byte.signed_gt_lanes::<I64>(0x100_u128)
                .extract_lane::<I8>(0)
                .unsigned()
                .extend::<I32>(),
            // A high result byte depends on the low byte's carry.
            byte.add_lanes::<I16>(1_u128)
                .extract_lane::<I8>(1)
                .unsigned()
                .extend::<I32>(),
            // A byte replacement overlaps, but does not replace, this whole word.
            overlapping
                .extract_lane::<I16>(0)
                .unsigned()
                .extend::<I32>(),
            input
                .replace_lane::<I16>(7, 0xabcd)
                .extract_lane::<I8>(0)
                .unsigned()
                .extend::<I32>(),
            input
                .replace_lane::<I32>(0, 0xdead_beef_u32)
                .replace_lane::<I16>(0, 0x1234)
                .extract_lane::<I32>(0),
        ];
        for (index, result) in results.into_iter().enumerate() {
            body.store(memory, 16 + index as u32 * 4, result)?;
        }
        body.store(
            memory,
            40,
            high.add_lanes::<I8>(0x0101_0101_0101_0101_0101_0101_0101_0101_u128)
                .extract_lane::<I64>(1),
        )?;
        body.store(
            memory,
            48,
            moved
                .add_lanes::<I16>(0x0001_0001_0001_0001_u128)
                .extract_lane::<I64>(0),
        )?;
        body.store(
            memory,
            56,
            input.replace_lane(1, input.extract_lane::<I64>(1)),
        )?;
        body.store(
            memory,
            72,
            input
                .replace_lane::<I8>(0, 0xab)
                .replace_lane::<I16>(0, 0x1234)
                .extract_lane::<I64>(0),
        )?;
        body.return_(())
    });
    let mut expected = initial.clone();
    for (index, value) in [0xff_u32, 0, 1, 0xabff, 0xff, 0xdead_1234]
        .into_iter()
        .enumerate()
    {
        expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    expected[40..48].copy_from_slice(&0x1335_5779_91ac_cef0_u64.to_le_bytes());
    expected[48..56].copy_from_slice(&0x1235_5679_90ac_cdf0_u64.to_le_bytes());
    expected[56..72].copy_from_slice(&initial[..16]);
    expected[72..80].copy_from_slice(&0x0100_0000_0000_1234_u64.to_le_bytes());
    check(&module, &initial, &expected, 0, v8);
}

fn branch_facts(v8: bool) {
    let mut initial = vec![0xa5; 40];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let enabled = body.parameter::<I32>(0)?.ne(0);
        let input = body.load::<V128>(memory, 0)?;
        let other = enabled.select(&input, 0_u128);
        let difference = input.sub_lanes::<I16>(&other);
        let compared = input.eq_lanes::<I32>(other);
        let result = Val::<V128>::from(0_u128)
            .replace_lane(0, compared.extract_lane::<I64>(0))
            .add_lanes::<I16>(&difference)
            .extract_lane::<I64>(0);
        body.if_(enabled, |mut arm| {
            arm.store(memory, 16, difference)?;
            arm.store(memory, 32, result)?;
            arm.return_(())
        })?;
        body.return_(())
    });
    assert!(!operators(&module).iter().any(|op| matches!(
        op,
        Operator::I16x8Sub | Operator::I16x8Add | Operator::I32x4Eq
    )));
    check(&module, &initial, &initial, 0, v8);
    let mut expected = initial.clone();
    expected[16..32].fill(0);
    expected[32..40].fill(0xff);
    check(&module, &initial, &expected, 1, v8);
}

fn loop_and_high_result(v8: bool) {
    let mut initial = vec![0xa5; 32];
    initial[..8].copy_from_slice(&0x7ffe_fffe_8000_0001_u64.to_le_bytes());
    initial[8..16].copy_from_slice(&0x0003_0002_0001_ffff_u64.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let input = body.load::<I64>(memory, 0)?;
        let addend = body.load::<I64>(memory, 8)?;
        let twice = |input: Val<I64>| {
            let vector = Val::<V128>::from(0_u128).replace_lane(0, input);
            let right = Val::<V128>::from(0_u128).replace_lane(0, &addend);
            let sum = vector.add_lanes::<I16>(&right).extract_lane::<I64>(0);
            Val::<V128>::from(0_u128)
                .replace_lane(0, sum)
                .add_lanes::<I16>(right)
                .extract_lane::<I64>(0)
        };
        let initial = twice(input);
        // The first result channel is dead, but the high product still depends
        // on the rewritten packed value and must retain its component index.
        let (_, high) = initial.unsigned().mul_wide(u64::MAX);
        body.store(memory, 24, high)?;
        let result = body.loop_::<(I32, I64), I64>(
            (count, initial),
            |mut iteration, labels, (remaining, value)| {
                iteration.branch_if(remaining.eq(0), &labels.exit, &value)?;
                iteration.branch(&labels.again, (remaining.sub(1), twice(value)))
            },
        )?;
        body.store(memory, 16, result)?;
        body.return_(())
    });
    assert_eq!(
        operators(&module)
            .iter()
            .filter(|op| matches!(op, Operator::I64x2ExtractLane { .. }))
            .count(),
        2
    );
    for (count, result) in [
        (0, 0x8004_0002_8002_ffff_u64),
        (1, 0x800a_0006_8004_fffd),
        (3, 0x8016_000e_8008_fff9),
    ] {
        let mut expected = initial.clone();
        expected[16..24].copy_from_slice(&result.to_le_bytes());
        expected[24..32].copy_from_slice(&0x8004_0002_8002_fffe_u64.to_le_bytes());
        check(&module, &initial, &expected, count, v8);
    }
}

#[test]
fn packed_identities_fold_at_every_integer_width() {
    identities::<I8>(false);
    identities::<I16>(false);
    identities::<I32>(false);
    identities::<I64>(false);
}

#[test]
fn lane_folds_preserve_widths_and_consumers() {
    narrow_carriers(false);
    lane_dependencies(false);
    shared_vector(false, false);
    shared_vector(false, true);
    branch_facts(false);
    loop_and_high_result(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_vector_folds_preserve_lane_observations() {
    identities::<I8>(true);
    identities::<I16>(true);
    identities::<I32>(true);
    identities::<I64>(true);
    narrow_carriers(true);
    lane_dependencies(true);
    shared_vector(true, false);
    shared_vector(true, true);
    branch_facts(true);
    loop_and_high_result(true);
}
