//! Packed observations keep carries, comparison inputs, and complete consumers.

use super::*;

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
        // The complete sum must retain its zero upper input lanes in either
        // use order, even if the scalar observation bypasses reconstruction.
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
    let mut initial = vec![0xa5; 88];
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
            // Dropping the low byte would introduce a borrow into this byte.
            byte.sub_lanes::<I16>(1_u128)
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
            48,
            high.add_lanes::<I8>(0x0101_0101_0101_0101_0101_0101_0101_0101_u128)
                .extract_lane::<I64>(1),
        )?;
        body.store(
            memory,
            56,
            moved
                .add_lanes::<I16>(0x0001_0001_0001_0001_u128)
                .extract_lane::<I64>(0),
        )?;
        body.store(
            memory,
            64,
            input.replace_lane(1, input.extract_lane::<I64>(1)),
        )?;
        body.store(
            memory,
            80,
            input
                .replace_lane::<I8>(0, 0xab)
                .replace_lane::<I16>(0, 0x1234)
                .extract_lane::<I64>(0),
        )?;
        body.return_(())
    });
    let mut expected = initial.clone();
    for (index, value) in [0xff_u32, 0, 1, 0, 0xabff, 0xff, 0xdead_1234]
        .into_iter()
        .enumerate()
    {
        expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    expected[48..56].copy_from_slice(&0x1335_5779_91ac_cef0_u64.to_le_bytes());
    expected[56..64].copy_from_slice(&0x1235_5679_90ac_cdf0_u64.to_le_bytes());
    expected[64..80].copy_from_slice(&initial[..16]);
    expected[80..88].copy_from_slice(&0x0100_0000_0000_1234_u64.to_le_bytes());
    check(&module, &initial, &expected, 0, v8);
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

fn saturation_and_product_dependencies(v8: bool) {
    let mut initial = vec![0xa5; 40];
    initial[..16].copy_from_slice(&0x0000_0000_0003_fffe_0000_ffff_8000_7fff_u128.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let input = body.load::<V128>(memory, 0)?;
        let bytes = input.and(0x00ff_00ff_00ff_00ff_00ff_00ff_00ff_00ff_u128);
        let ones = 0x0001_0001_0001_0001_0001_0001_0001_0001_u128;
        let results = [
            // Saturation observes the high word bits even for a low-byte result.
            bytes
                .signed_saturating_add_lanes::<I16>(ones)
                .extract_lane::<I8>(0)
                .unsigned()
                .extend::<I32>(),
            bytes
                .signed_saturating_sub_lanes::<I16>(ones)
                .extract_lane::<I8>(2)
                .unsigned()
                .extend::<I32>(),
            bytes
                .unsigned_saturating_add_lanes::<I16>(ones)
                .extract_lane::<I8>(4)
                .unsigned()
                .extend::<I32>(),
            bytes
                .unsigned_saturating_sub_lanes::<I16>(0x100_u128)
                .extract_lane::<I8>(0)
                .unsigned()
                .extend::<I32>(),
            // These result lanes draw from different input positions or pairs.
            bytes
                .signed_widening_mul_high_lanes::<I16>(ones)
                .extract_lane::<I32>(0),
            bytes
                .replace_lane::<I16>(1, 2)
                .signed_dot_i16_pairs(ones)
                .extract_lane::<I8>(0)
                .unsigned()
                .extend::<I32>(),
        ];
        for (index, result) in results.into_iter().enumerate() {
            body.store(memory, 16 + index as u32 * 4, result)?;
        }
        body.return_(())
    });
    let mut expected = initial.clone();
    for (index, value) in [0_u32, 0xff, 0, 0, 0xfe, 1].into_iter().enumerate() {
        expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    check(&module, &initial, &expected, 0, v8);
}

#[test]
fn packed_observations_preserve_dependencies_and_complete_uses() {
    lane_dependencies(false);
    saturation_and_product_dependencies(false);
    shared_vector(false, false);
    shared_vector(false, true);
    loop_and_high_result(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_packed_observations_preserve_dependencies_and_complete_uses() {
    lane_dependencies(true);
    saturation_and_product_dependencies(true);
    shared_vector(true, false);
    shared_vector(true, true);
    loop_and_high_result(true);
}
