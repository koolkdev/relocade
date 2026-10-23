//! Partial observations may discard work only inside an unshared vector chain.

use super::*;
use crate::wasm::Call;
use relocade_compiler::{I16, I64, V64};
use wasmparser::{Operator, Parser, Payload};

fn operations(bytes: &[u8]) -> Vec<Operator<'_>> {
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
            _ => None,
        })
        .flat_map(|reader| reader.into_iter().map(Result::unwrap))
        .collect()
}

fn discarded_and_shared() {
    let mut initial = vec![0xa5; 80];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    initial[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    for shared in [false, true] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &initial);
        let module = fixture.function(&[Type::I32], &[], move |mut body| {
            let x = body.load::<V128>(memory, 0)?;
            let y = body.load::<V128>(memory, 16)?;
            // The unused replacement includes scalar work that selection can retire.
            let word = body.parameter::<I32>(0)?.mul(17);
            let result = x.replace_lane(3, word).add_lanes::<I32>(&y);
            if shared {
                body.store(memory, 32, &result)?;
            }
            body.store(memory, 48, result.extract_lane::<I32>(0))?;
            let low = x
                .low_half()
                .replace_lane::<I16>(3, 5)
                .sub_lanes::<I16>(y.low_half());
            body.store(memory, 52, low.extract_lane::<I16>(0))?;
            body.return_(())
        });
        let ops = operations(module.bytes());
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, Operator::I32x4ReplaceLane { .. }))
                .count(),
            usize::from(shared)
        );
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, Operator::I32Mul))
                .count(),
            usize::from(shared)
        );
        assert!(!ops
            .iter()
            .any(|op| matches!(op, Operator::I16x8ReplaceLane { .. })));
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, Operator::I32x4Add))
                .count(),
            1
        );
        for input in [0_u32, 1, 0xffff_ffff] {
            let mut expected = initial.clone();
            let a = LEFT.to_le_bytes();
            let b = RIGHT.to_le_bytes();
            if shared {
                for lane in 0..4 {
                    let left = if lane == 3 {
                        input.wrapping_mul(17)
                    } else {
                        u32::from_le_bytes(a[lane * 4..lane * 4 + 4].try_into().unwrap())
                    };
                    let right = u32::from_le_bytes(b[lane * 4..lane * 4 + 4].try_into().unwrap());
                    expected[32 + lane * 4..36 + lane * 4]
                        .copy_from_slice(&left.wrapping_add(right).to_le_bytes());
                }
            }
            expected[48..52]
                .copy_from_slice(&(LEFT as u32).wrapping_add(RIGHT as u32).to_le_bytes());
            expected[52..54]
                .copy_from_slice(&(LEFT as u16).wrapping_sub(RIGHT as u16).to_le_bytes());
            check(&module, &expected, input as i32);
        }
    }
}

fn lane_dependencies() {
    let mut initial = vec![0xa5; 64];
    initial[..8].copy_from_slice(&0x1200_u64.to_le_bytes());
    for case in 0..7 {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &initial);
        let module = fixture.function(&[Type::I32], &[], move |mut body| {
            let x = body.load::<V64>(memory, 0)?;
            let byte = body.parameter::<I32>(0)?.truncate::<I8>();
            let low = x.replace_lane(0, &byte);
            let high = x.replace_lane(1, byte);
            let value = match case {
                0 => low.add_lanes::<I16>(1_u64).extract_lane::<I8>(1),
                1 => low.sub_lanes::<I16>(1_u64).extract_lane::<I8>(1),
                2 => low.mul_lanes::<I16>(3_u64).extract_lane::<I8>(1),
                3 => high
                    .unsigned_saturating_add_lanes::<I16>(0x0100_u64)
                    .extract_lane::<I8>(0),
                4 => high.eq_lanes::<I16>(0x1200_u64).extract_lane::<I8>(0),
                5 => high.unsigned_shr_lanes::<I16>(8).extract_lane::<I8>(0),
                6 => high
                    .signed_widening_mul_lanes::<I16>(2_u64)
                    .extract_lane::<I8>(1),
                _ => unreachable!(),
            };
            body.store(memory, 16, value)?;
            body.return_(())
        });
        for input in [0_u8, 1, 0x12, 0x7f, 0x80, 0xff] {
            let low = 0x1200_u16 | u16::from(input);
            let high = u16::from(input) << 8;
            let result = match case {
                0 => (low.wrapping_add(1) >> 8) as u8,
                1 => (low.wrapping_sub(1) >> 8) as u8,
                2 => (low.wrapping_mul(3) >> 8) as u8,
                3 => high.saturating_add(0x100) as u8,
                4 => {
                    if high == 0x1200 {
                        255
                    } else {
                        0
                    }
                }
                5 => input,
                6 => ((i32::from(high as i16) * 2) >> 8) as u8,
                _ => unreachable!(),
            };
            let mut expected = initial.clone();
            expected[16] = result;
            check(&module, &expected, i32::from(input));
        }
    }
}

fn surviving_wide_product() {
    let initial = LEFT.to_le_bytes();
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I64], &[Type::I32, Type::I64], move |mut body| {
        let input = body.parameter::<I64>(0)?;
        let (low, high) = input.unsigned().mul_wide(u64::MAX);
        let x = body.load::<V128>(memory, 0)?;
        let observed = x
            .replace_lane(1, low)
            .add_lanes::<I32>(1_u128)
            .extract_lane::<I32>(0);
        body.return_((observed, high))
    });
    assert!(!operations(module.bytes())
        .iter()
        .any(|op| matches!(op, Operator::I64x2ReplaceLane { .. })));
    for input in [0_u64, 1, 17, u64::MAX] {
        let arguments = [Value::I64(input as i64)];
        let expected = [
            Value::I32((LEFT as u32).wrapping_add(1) as i32),
            Value::I64(input.saturating_sub(1) as i64),
        ];

        assert_eq!(
            module.instantiate().call_values("run", &arguments).unwrap(),
            expected
        );
    }
}

fn retired_boolean_keeps_numeric_mask() {
    let mut initial = vec![0xa5; 32];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let byte = body.parameter::<I32>(0)?.truncate::<I8>();
        let numeric = byte.unsigned().extend::<I32>();
        let predicate = byte.eq(0).unsigned().extend::<I8>();
        let vector = body.load::<V128>(memory, 0)?;
        let observed = vector
            .replace_lane(15, predicate)
            .add_lanes::<I8>(1_u128)
            .extract_lane::<I8>(0);
        body.store(memory, 16, numeric)?;
        body.store(memory, 20, observed)?;
        body.return_(())
    });
    for input in [0, 1, 0x80, 0xff, 0x180] {
        let mut expected = initial.clone();
        expected[16..20].copy_from_slice(&(input as u32 & 255).to_le_bytes());
        expected[20] = (LEFT as u8).wrapping_add(1);
        check(&module, &expected, input);
    }
}

#[test]
fn partial_shuffle_observations_retain_only_contributing_replacements() {
    let mut initial = vec![0xa5; 40];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    initial[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let word = body.parameter::<I32>(0)?;
        let left = body
            .load::<V128>(memory, 0)?
            .replace_lane(2, word.mul(17))
            .replace_lane(3, &word);
        let right = body
            .load::<V128>(memory, 16)?
            .replace_lane(0, word.mul(19))
            .replace_lane(1, word.add(1));
        // Mix original and replaced bytes from both inputs.
        let indices = [0, 20, 14, 26, 8, 9, 10, 11, 16, 17, 18, 19, 0, 1, 2, 3];
        let observed = left
            .shuffle_bytes(right, indices)
            .xor(0x0807_0605_u128)
            .extract_lane::<I32>(0);
        body.store(memory, 32, observed)?;
        body.return_(())
    });
    let ops = operations(module.bytes());
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::I32x4ReplaceLane { .. }))
            .count(),
        2,
        "only replacements contributing to the shuffled word remain: {ops:?}"
    );
    assert!(!ops.iter().any(|op| matches!(op, Operator::I32Mul)));
    for input in [0_u32, 0xffff_ffff, 0x1234_5678, 0x80ff_ff00] {
        let left = input.to_le_bytes();
        let right = input.wrapping_add(1).to_le_bytes();
        let mut expected = initial.clone();
        expected[32..36].copy_from_slice(&[0x10 ^ 5, right[0] ^ 6, left[2] ^ 7, 0xaa ^ 8]);
        check(&module, &expected, input as i32);
    }
}

#[test]
fn discarded_replacements_keep_loads_calls_and_effect_order() {
    let mut initial = vec![0xa5; 32];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let produce = fixture.callback("produce", signature(&[], &[Type::I32]), &[Value::I32(7)]);
    let module = fixture.function(&[], &[], move |mut body| {
        let vector = body.load::<V128>(memory, 0)?;
        let loaded = body.load::<I32>(memory, 16)?;
        body.store::<I32>(memory, 16, 0x1020_3040_u32)?;
        let called = body.call::<I32>(produce, &[])?;
        let observed = vector
            .replace_lane(2, loaded.mul(17))
            .replace_lane(3, called.mul(19))
            .add_lanes::<I32>(1_u128)
            .extract_lane::<I32>(0);
        body.store(memory, 20, observed)?;
        body.return_(())
    });
    let ops = operations(module.bytes());
    assert!(!ops
        .iter()
        .any(|op| matches!(op, Operator::I32Mul | Operator::I32x4ReplaceLane { .. })));
    let load = ops
        .iter()
        .position(|op| matches!(op, Operator::I32Load { memarg } if memarg.offset == 16))
        .unwrap();
    let store = ops
        .iter()
        .position(|op| matches!(op, Operator::I32Store { memarg } if memarg.offset == 16))
        .unwrap();
    let call = ops
        .iter()
        .position(|op| matches!(op, Operator::Call { .. }))
        .unwrap();
    assert!(load < store && store < call);
    let mut snapshot = initial.clone();
    snapshot[16..20].copy_from_slice(&0x1020_3040_u32.to_le_bytes());
    let calls = [Call::new("produce", &[]).with_memories(&[MemoryBytes::new("state", &snapshot)])];
    let mut expected = snapshot;
    expected[20..24].copy_from_slice(&(LEFT as u32).wrapping_add(1).to_le_bytes());

    let mut instance = module.instantiate();
    instance.call::<()>(()).unwrap();
    assert_eq!(instance.callbacks(), calls);
    assert_eq!(&instance.memory("state")[..expected.len()], expected);
}

#[test]
fn partial_vector_observations_preserve_sharing_dependencies_and_other_results() {
    discarded_and_shared();
    lane_dependencies();
    surviving_wide_product();
    retired_boolean_keeps_numeric_mask();
}
