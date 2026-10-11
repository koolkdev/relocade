//! Partial scalar and vector observations use the same demanded-bit specialization.

use super::*;
use wasm86_compiler::{I16, I64};

fn shared_partial_vector(v8: bool) {
    let mut initial = vec![0xa5; 48];
    initial[..16].copy_from_slice(&0x0001_0002_0003_0004_0005_0006_0007_0008_u128.to_le_bytes());
    initial[16..32].copy_from_slice(&0x0001_0002_0003_0004_0005_9999_0007_aaaa_u128.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let left = body.load::<V128>(memory, 0)?;
        let right = body.load::<V128>(memory, 16)?;
        let compared = left.eq_lanes::<I16>(&right);
        let restored = Val::<V128>::from(0_u128).replace_lane(0, compared.extract_lane::<I64>(0));
        let sum = restored.add_lanes::<I16>(right);
        // Both users observe the lower half. Neither needs the restored zeros.
        body.store(memory, 32, sum.extract_lane::<I16>(1))?;
        body.store(memory, 40, sum.extract_lane::<I64>(0))?;
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
        .any(|op| matches!(op, Operator::I64x2ReplaceLane { .. })));
    let mut expected = initial.clone();
    expected[32..34].copy_from_slice(&6_u16.to_le_bytes());
    expected[40..48].copy_from_slice(&0x0004_9999_0006_aaaa_u64.to_le_bytes());
    check(&module, &initial, &expected, 0, v8);
}

fn scalar_carry_and_narrow_lane(v8: bool) {
    let mut initial = vec![0xa5; 40];
    initial[..4].copy_from_slice(&0x1234_00ff_u32.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let input = body.load::<I32>(memory, 0)?;
        // Only the final byte matters, so the mask before the add is redundant.
        let low = input
            .and(0xff)
            .add(1)
            .truncate::<I8>()
            .unsigned()
            .extend::<I32>();
        body.store(memory, 8, low)?;
        // Replacement observes eight bits but must retain its logical lane width
        // when its scalar operand's physical i32 carrier has dirty upper bits.
        let byte = input.truncate::<I8>().add(3);
        let replaced = Val::<V128>::from(0_u128).replace_lane(15, byte);
        body.store(memory, 16, replaced)?;
        body.return_(())
    });
    assert_eq!(
        operators(&module)
            .iter()
            .filter(|op| matches!(op, Operator::I32And))
            .count(),
        1
    );
    let mut expected = initial.clone();
    expected[8..12].fill(0);
    expected[16..32].fill(0);
    expected[31] = 2;
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
        let sum = restored.add_lanes::<I16>(1_u128);
        let joined = body.if_value::<V128>(
            body.parameter::<I32>(0)?.ne(0),
            |mut arm| {
                arm.store(memory, 16, sum.extract_lane::<I64>(0))?;
                arm.yield_(&sum)
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
        let sum = (low & !0xffff) | u64::from((low as u16).wrapping_add(1));
        if count != 0 {
            expected[16..24].copy_from_slice(&sum.to_le_bytes());
        }
        expected[24..40]
            .copy_from_slice(&(if count == 0 { LEFT } else { u128::from(sum) }).to_le_bytes());
        expected[40..56].copy_from_slice(&u128::from(low).to_le_bytes());
        check(&module, &initial, &expected, count, v8);
    }
}

#[test]
fn demanded_bits_preserve_shared_values_and_carriers() {
    shared_partial_vector(false);
    scalar_carry_and_narrow_lane(false);
    full_observation_after_join(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_demanded_bits_preserve_shared_values_and_carriers() {
    shared_partial_vector(true);
    scalar_carry_and_narrow_lane(true);
    full_observation_after_join(true);
}
