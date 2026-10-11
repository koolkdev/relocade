//! Shared recipes can have both partial and complete vector consumers.
use super::*;
use wasm86_compiler::I64;

fn shared_partial_vector(v8: bool) {
    let mut initial = vec![0xa5; 48];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    initial[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let left = body.load::<V128>(memory, 0)?;
        let right = body.load::<V128>(memory, 16)?;
        let masked = left.and(&right);
        let restored = Val::<V128>::from(0_u128).replace_lane(0, masked.extract_lane::<I64>(0));
        let result = restored.xor(right);
        body.store(memory, 32, result.extract_lane::<I32>(1))?;
        body.store(memory, 40, result.extract_lane::<I64>(0))?;
        body.return_(())
    });
    let ops = operators(&module);
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::V128Xor))
            .count(),
        1
    );
    assert!(!ops
        .iter()
        .any(|op| matches!(op, Operator::I64x2ReplaceLane { .. })));
    let mut expected = initial.clone();
    let bits = (LEFT & RIGHT) ^ RIGHT;
    expected[32..36].copy_from_slice(&((bits >> 32) as u32).to_le_bytes());
    expected[40..48].copy_from_slice(&(bits as u64).to_le_bytes());
    check(&module, &initial, &expected, 0, v8);
}

fn narrowed_scalar_lane(v8: bool) {
    let mut initial = vec![0xa5; 32];
    initial[..8].copy_from_slice(&0x12345678_ffffffff_u64.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let input = body.load::<I64>(memory, 0)?;
        let word = input.truncate::<I32>().add(3);
        let replaced = Val::<V128>::from(0_u128).replace_lane(3, word);
        body.store(memory, 16, replaced)?;
        body.return_(())
    });
    let mut expected = initial.clone();
    expected[16..32].copy_from_slice(&(2_u128 << 96).to_le_bytes());
    check(&module, &initial, &expected, 0, v8);
}

fn full_observation_after_join(v8: bool) {
    let mut initial = vec![0xa5; 56];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let input = body.load::<V128>(memory, 0)?;
        let restored = Val::<V128>::from(0_u128).replace_lane(0, input.extract_lane::<I64>(0));
        let updated = restored.or(1_u128);
        let joined = body.if_value::<V128>(
            body.parameter::<I32>(0)?.ne(0),
            |mut arm| {
                arm.store(memory, 16, updated.extract_lane::<I64>(0))?;
                arm.yield_(&updated)
            },
            |arm| arm.yield_(&input),
        )?;
        body.store(memory, 24, joined)?;
        body.store(memory, 40, restored)?;
        body.return_(())
    });
    for count in [0, 1] {
        let mut expected = initial.clone();
        let low = LEFT as u64;
        if count != 0 {
            expected[16..24].copy_from_slice(&(low | 1).to_le_bytes());
        }
        expected[24..40].copy_from_slice(
            &(if count == 0 {
                LEFT
            } else {
                u128::from(low | 1)
            })
            .to_le_bytes(),
        );
        expected[40..56].copy_from_slice(&u128::from(low).to_le_bytes());
        check(&module, &initial, &expected, count, v8);
    }
}

fn later_snapshot_cannot_supply_an_earlier_observation(v8: bool, nested_first: bool) {
    let mut initial = vec![0xa5; 72];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    initial[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let input = body.load::<V128>(memory, 0)?;
        let low = input.and(u128::from(u64::MAX)).extract_lane::<I64>(0);
        body.store(memory, 32, if nested_first { low.add(1) } else { low })?;
        let later = body.load::<V128>(memory, 16)?;
        let complete = input.replace_lane(1, later.extract_lane::<I64>(1));
        // This equivalent extraction is useful after the second load. It must
        // not replace the earlier root, or a dependency of the earlier add.
        body.store(memory, 48, &complete)?;
        body.store(memory, 64, complete.extract_lane::<I64>(0))?;
        body.return_(())
    });
    let mut expected = initial.clone();
    expected[32..40].copy_from_slice(
        &(LEFT as u64)
            .wrapping_add(u64::from(nested_first))
            .to_le_bytes(),
    );
    expected[48..56].copy_from_slice(&(LEFT as u64).to_le_bytes());
    expected[56..64].copy_from_slice(&((RIGHT >> 64) as u64).to_le_bytes());
    expected[64..72].copy_from_slice(&(LEFT as u64).to_le_bytes());
    check(&module, &initial, &expected, 0, v8);
}

#[test]
fn partial_vector_consumers_preserve_sharing_and_complete_uses() {
    shared_partial_vector(false);
    narrowed_scalar_lane(false);
    full_observation_after_join(false);
    later_snapshot_cannot_supply_an_earlier_observation(false, false);
    later_snapshot_cannot_supply_an_earlier_observation(false, true);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_partial_vector_consumers_preserve_sharing_and_complete_uses() {
    shared_partial_vector(true);
    narrowed_scalar_lane(true);
    full_observation_after_join(true);
    later_snapshot_cannot_supply_an_earlier_observation(true, false);
    later_snapshot_cannot_supply_an_earlier_observation(true, true);
}
