//! Packed identities and lane round trips preserve logical widths and path facts.

use super::*;

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

#[test]
fn packed_identities_and_lane_carriers() {
    identities::<I8>(false);
    identities::<I16>(false);
    identities::<I32>(false);
    identities::<I64>(false);
    narrow_carriers(false);
    branch_facts(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_packed_identities_and_lane_carriers() {
    identities::<I8>(true);
    identities::<I16>(true);
    identities::<I32>(true);
    identities::<I64>(true);
    narrow_carriers(true);
    branch_facts(true);
}
