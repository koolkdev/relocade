//! Scalar and vector observations share scoped bit facts and exact identities.

use super::*;
use wasm86_compiler::{I16, I64};

fn exact_reassembly(v8: bool) {
    let mut initial = vec![0xa5; 48];
    initial[..8].copy_from_slice(&0x7ffe_fffe_8000_0001_u64.to_le_bytes());
    initial[8..16].copy_from_slice(&0x0003_0002_0001_ffff_u64.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let left = Val::<V128>::from(0_u128).replace_lane(0, body.load::<I64>(memory, 0)?);
        let right = Val::<V128>::from(0_u128).replace_lane(0, body.load::<I64>(memory, 8)?);
        let sum = left.add_lanes::<I16>(right);
        let restored = Val::<V128>::from(0_u128).replace_lane(0, sum.extract_lane::<I64>(0));
        // Both full-vector consumers agree, so sharing needs no use-count rule.
        body.store(memory, 16, restored)?;
        body.store(memory, 32, sum)?;
        body.return_(())
    });
    let ops = operators(&module);
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I16x8Add))
            .count(),
        1
    );
    assert!(!ops
        .iter()
        .any(|op| matches!(op, Operator::I64x2ExtractLane { .. })));
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I64x2ReplaceLane { .. }))
            .count(),
        2
    );
    let mut expected = initial.clone();
    for offset in [16, 32] {
        expected[offset..offset + 16].copy_from_slice(&0x8001_0000_8001_0000_u128.to_le_bytes());
    }
    check(&module, &initial, &expected, 0, v8);
}

fn vector_join(v8: bool) {
    let mut initial = vec![0xa5; 56];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    initial[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let choose = body.parameter::<I32>(0)?.ne(0);
        let left = body.load::<V128>(memory, 0)?;
        let right = body.load::<V128>(memory, 16)?;
        let joined = body.if_value::<V128>(
            choose,
            |arm| arm.yield_(left.replace_lane::<I64>(1, 0x55aa_1234_5678_9abc_u64)),
            |arm| arm.yield_(right.replace_lane::<I64>(1, 0x55aa_1234_5678_9abc_u64)),
        )?;
        body.store(memory, 32, &joined)?;
        body.store(memory, 48, joined.extract_lane::<I64>(1))?;
        body.return_(())
    });
    assert!(!operators(&module)
        .iter()
        .any(|op| matches!(op, Operator::I64x2ExtractLane { .. })));
    for (count, low) in [(0, RIGHT as u64), (1, LEFT as u64)] {
        let mut expected = initial.clone();
        expected[32..40].copy_from_slice(&low.to_le_bytes());
        expected[40..48].copy_from_slice(&0x55aa_1234_5678_9abc_u64.to_le_bytes());
        expected[48..56].copy_from_slice(&0x55aa_1234_5678_9abc_u64.to_le_bytes());
        check(&module, &initial, &expected, count, v8);
    }
}

fn branch_observation(v8: bool) {
    for high in [0_u64, 1, u64::MAX] {
        let mut initial = vec![0xa5; 64];
        initial[..8].copy_from_slice(&0x0123_4567_89ab_cdef_u64.to_le_bytes());
        initial[8..16].copy_from_slice(&high.to_le_bytes());
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &initial);
        let module = fixture.function(&[Type::I32], &[], |mut body| {
            let input = body.load::<V128>(memory, 0)?;
            let restored = Val::<V128>::from(0_u128).replace_lane(0, input.extract_lane::<I64>(0));
            body.if_else(
                input.extract_lane::<I64>(1).eq(0),
                |mut arm| {
                    arm.store(memory, 16, &restored)?;
                    Ok(())
                },
                |mut arm| {
                    arm.store(memory, 32, &restored)?;
                    Ok(())
                },
            )?;
            body.store(memory, 48, restored)?;
            body.return_(())
        });
        let mut expected = initial.clone();
        let offset = if high == 0 { 16 } else { 32 };
        expected[offset..offset + 16].copy_from_slice(&0x0123_4567_89ab_cdef_u128.to_le_bytes());
        expected[48..64].copy_from_slice(&0x0123_4567_89ab_cdef_u128.to_le_bytes());
        check(&module, &initial, &expected, 0, v8);
    }
}

fn vector_backedge(v8: bool) {
    let initial = vec![0xa5; 24];
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let initial = Val::<V128>::from(0_u128).replace_lane(0, body.load::<I64>(memory, 0)?);
        let result = body.loop_::<(I32, V128), V128>(
            (count, initial),
            |mut iteration, labels, (remaining, value)| {
                iteration.branch_if(remaining.eq(0), &labels.exit, &value)?;
                iteration.branch(
                    &labels.again,
                    (remaining.sub(1), value.xor(u128::from(u64::MAX) << 64)),
                )
            },
        )?;
        body.store(memory, 8, result)?;
        body.return_(())
    });
    for count in [0, 1, 2, 5] {
        let mut expected = initial.clone();
        expected[16..24].fill(if count % 2 == 0 { 0 } else { 0xff });
        check(&module, &initial, &expected, count, v8);
    }
}

#[test]
fn shared_bit_facts_preserve_vector_observations() {
    exact_reassembly(false);
    vector_join(false);
    branch_observation(false);
    vector_backedge(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_shared_bit_facts_preserve_vector_observations() {
    exact_reassembly(true);
    vector_join(true);
    branch_observation(true);
    vector_backedge(true);
}
