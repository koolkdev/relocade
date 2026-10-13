//! Overlapping lane facts follow ordinary branch, join and snapshot lifetimes.

use super::*;
use crate::wasm::MemoryBytes;
use wasm86_compiler::{Val, I16, I64, V128, V64};

fn guards_and_switch(v8: bool) {
    for masked in [false, true] {
        let module = Fixture::new().function(&[Type::I64], &[Type::I32], |mut body| {
            let vector = Val::<V64>::from_bits(body.parameter::<I64>(0)?);
            let high = vector.extract_lane::<I16>(1).unsigned().extend::<I32>();
            let word = vector.extract_lane::<I32>(0);
            let guard = if masked {
                word.and(0xffff0000_u32).ne(0x12340000)
            } else {
                word.ne(0x12345678)
            };
            body.if_(guard, |arm| arm.return_(7))?;
            body.return_(high)
        });
        assert_eq!(
            count(&module, |op| matches!(
                op,
                Operator::I16x8ExtractLaneU { .. }
            )),
            0
        );
        for input in [0_u64, 0x12345678, 0x12345679, 0x98765678, u64::MAX] {
            let matches = if masked {
                input as u32 & 0xffff0000 == 0x12340000
            } else {
                input as u32 == 0x12345678
            };
            check_result(
                &module,
                &[Value::I64(input as i64)],
                &[Value::I32(if matches { 0x1234 } else { 7 })],
                v8,
            );
        }
    }
    let module = Fixture::new().function(&[Type::I64], &[Type::I32], |mut body| {
        let vector = Val::<V64>::from_bits(body.parameter::<I64>(0)?);
        body.switch(
            vector.extract_lane::<I32>(0),
            &[0x12345678, 0x76543210],
            |arm, key| {
                if key.is_some() {
                    arm.return_(vector.extract_lane::<I16>(1).unsigned().extend::<I32>())
                } else {
                    arm.return_(-1)
                }
            },
        )?;
        body.return_(-2)
    });
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I16x8ExtractLaneU { .. }
        )),
        0
    );
    for (input, expected) in [(0x12345678, 0x1234), (0x76543210, 0x7654), (0, -1)] {
        check_result(&module, &[Value::I64(input)], &[Value::I32(expected)], v8);
    }
}

fn joins_and_siblings(v8: bool) {
    let module = Fixture::new().function(
        &[Type::I32, Type::I64, Type::I64],
        &[Type::I32],
        |mut body| {
            let mode = body.parameter::<I32>(0)?.ne(0);
            let a = Val::<V64>::from_bits(body.parameter::<I64>(1)?);
            let b = Val::<V64>::from_bits(body.parameter::<I64>(2)?);
            let joined = body.if_value::<V64>(
                mode,
                |mut arm| {
                    arm.if_(a.extract_lane::<I32>(0).ne(0x12345678), |exit| {
                        exit.return_(-1)
                    })?;
                    arm.yield_(a)
                },
                |mut arm| {
                    arm.if_(b.extract_lane::<I32>(0).ne(0x12345679), |exit| {
                        exit.return_(-2)
                    })?;
                    arm.yield_(b)
                },
            )?;
            body.return_(joined.extract_lane::<I16>(1).unsigned().extend::<I32>())
        },
    );
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I16x8ExtractLaneU { .. }
        )),
        0
    );
    for (mode, a, b, expected) in [
        (1, 0x12345678, 0, 0x1234),
        (0, 0, 0x12345679, 0x1234),
        (1, 0, 0x12345679, -1),
        (0, 0x12345678, 0, -2),
    ] {
        check_result(
            &module,
            &[Value::I32(mode), Value::I64(a), Value::I64(b)],
            &[Value::I32(expected)],
            v8,
        );
    }
    let module = Fixture::new().function(&[Type::I64], &[Type::I32], |mut body| {
        let vector = Val::<V64>::from_bits(body.parameter::<I64>(0)?);
        let high = vector.extract_lane::<I16>(1).unsigned().extend::<I32>();
        let result = body.if_value::<I32>(
            vector.extract_lane::<I32>(0).eq(0x12345678),
            |arm| arm.yield_(&high),
            |arm| arm.yield_(high.add(1)),
        )?;
        body.return_(result.add(high))
    });
    for (input, expected) in [
        (0x12345678, 0x2468),
        (0x12345679, 0x2469),
        (0x98765678, 0x130ed),
        (0, 1),
    ] {
        check_result(&module, &[Value::I64(input)], &[Value::I32(expected)], v8);
    }
}

fn loop_and_snapshots(v8: bool) {
    let module = Fixture::new().function(&[], &[Type::I32], |mut body| {
        let sum = body.loop_::<(I32, V64, I32), I32>(
            (0, 1_u64, 0),
            |mut iteration, labels, (index, vector, sum)| {
                iteration.branch_if(index.unsigned().ge(3), &labels.exit, &sum)?;
                let lane = vector.extract_lane::<I16>(0).unsigned().extend::<I32>();
                let term = iteration.if_value::<I32>(
                    vector.extract_lane::<I32>(0).eq(1),
                    |arm| arm.yield_(&lane),
                    |arm| arm.yield_(lane.add(10)),
                )?;
                iteration.branch(
                    &labels.again,
                    (index.add(1), vector.add_lanes::<I16>(1_u64), sum.add(term)),
                )
            },
        )?;
        body.return_(sum)
    });
    check_result(&module, &[], &[Value::I32(26)], v8);

    let initial = 0x1234_5678_90ab_cdef_1234_5678_90ab_cdef_u128.to_le_bytes();
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[], &[Type::I32; 2], |mut body| {
        let before = body.load::<V128>(memory, 0)?;
        body.if_(before.extract_lane::<I32>(3).ne(0x12345678), |arm| {
            arm.return_((0, 0))
        })?;
        body.store::<I32>(memory, 12, 0xdeadbeef_u32)?;
        let after = body.load::<V128>(memory, 0)?;
        body.return_((
            before.extract_lane::<I16>(7).unsigned().extend::<I32>(),
            after.extract_lane::<I16>(7).unsigned().extend::<I32>(),
        ))
    });
    let returned = [Value::I32(0x1234), Value::I32(0xdead)];
    let mut expected = initial;
    expected[12..].copy_from_slice(&0xdeadbeef_u32.to_le_bytes());
    if v8 {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", &initial)])
            ),
            Observation::returned(&returned).with_memories(&[MemoryBytes::new("state", &expected)])
        );
    } else {
        let mut instance = module.instantiate();
        assert_eq!(instance.call_values("run", &[]).unwrap(), returned);
        assert_eq!(&instance.memory("state")[..16], expected);
    }
}

#[test]
fn vector_lane_observations_share_facts_only_on_their_established_paths() {
    guards_and_switch(false);
    joins_and_siblings(false);
    loop_and_snapshots(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_vector_lane_observations_share_facts_only_on_their_established_paths() {
    guards_and_switch(true);
    joins_and_siblings(true);
    loop_and_snapshots(true);
}
