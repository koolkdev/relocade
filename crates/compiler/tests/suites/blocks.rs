use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, Callback, Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{Type, I1, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, ValType, Validator};

fn shared_exits() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0xa5, 0xa5, 0xa5]);
    fixture.function(
        &[Type::I1, Type::I1, Type::I1, Type::I32],
        Some(Type::I64),
        |mut body| {
            let unaligned = body.parameter::<I1>(0)?;
            let crossing = body.parameter::<I1>(1)?;
            let denied = body.parameter::<I1>(2)?;
            let address = body.parameter::<I32>(3)?;
            let before = body.load::<I32>(state, 0)?;
            let (scattered, physical) = body.block::<(I1, I32)>(|mut access, success| {
                let (fault_address, present) = access.block::<(I32, I1)>(|mut guards, fault| {
                    guards.if_(&unaligned, |mut unaligned| {
                        unaligned.if_(&crossing, |mut crossing| {
                            crossing.store::<I32>(state, 0, 11)?;
                            crossing.if_(&denied, |failure| {
                                failure.branch(&fault, (address.add(4096), true))
                            })?;
                            crossing.branch(&success, (true, address.add(100)))
                        })
                    })?;
                    guards.if_(&denied, |failure| failure.branch(&fault, (&address, false)))?;
                    guards.branch(&success, (false, address.add(200)))
                })?;
                access.store::<I32>(state, 4, 0xf00d)?;
                access.return_(
                    fault_address
                        .unsigned()
                        .extend::<I64>()
                        .or(present.unsigned().extend::<I64>().shl(32)),
                )
            })?;
            body.store(state, 4, scattered.unsigned().extend::<I32>())?;
            body.return_(physical.add(before).unsigned().extend::<I64>())
        },
    )
}

fn projected_results(observe: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5, 0x5a]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], Some(Type::I32)),
        Some(Value::I32(23)),
    );
    fixture.function(&[Type::I1], Some(Type::I32), |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let (word, _unused, flag) = body.if_value::<(I32, I64, I1)>(
            &condition,
            |mut arm| {
                arm.store::<I8>(state, 0, 1)?;
                let _unused_call = arm.call::<I32>(receive, &[9.into()])?;
                let unused_read = arm.load::<I64>(state, 65536)?;
                arm.yield_((5, unused_read, true))
            },
            |mut arm| {
                let unused_read = arm.load::<I64>(state, 65536)?;
                arm.yield_((9, unused_read, false))
            },
        )?;
        if observe {
            body.return_(word.add(flag.unsigned().extend::<I32>()))
        } else {
            body.return_(17)
        }
    })
}

fn nested_results() -> TestModule {
    Fixture::new().function(&[Type::I1], Some(Type::I64), |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let (word, (flag, wide)) = body.block::<(I32, (I1, I64))>(|mut outer, _| {
            let result = outer.if_value::<(I32, (I1, I64))>(
                &condition,
                |arm| arm.yield_((0x8000_0000u32, (true, u64::MAX))),
                |arm| arm.yield_((7, (false, 0x8000_0000_0000_0000u64))),
            )?;
            outer.yield_(result)
        })?;
        body.return_(
            wide.add(word.unsigned().extend::<I64>())
                .add(flag.unsigned().extend::<I64>()),
        )
    })
}

fn unused_pure_call() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5, 0x5a]);
    let helper = fixture
        .program
        .function(signature(&[], Some(Type::I64)), |body| body.trap())
        .unwrap();
    fixture.function(&[], Some(Type::I32), |mut body| {
        let (word, _unused) = body.block::<(I32, I64)>(|mut block, _| {
            block.store::<I8>(state, 0, 1)?;
            let unused = block.call::<I64>(helper, &[])?;
            block.yield_((7, unused))
        })?;
        body.return_(word)
    })
}

fn switch_exits(keys: &[u32]) -> TestModule {
    Fixture::new().function(&[Type::I32, Type::I1], Some(Type::I64), |mut body| {
        let selector = body.parameter::<I32>(0)?;
        let leave = body.parameter::<I1>(1)?;
        let (word, wide) = body.block::<(I32, I64)>(|mut block, exit| {
            let result = block.switch_value::<(I32, I64), _>(&selector, keys, |mut arm, key| {
                let word =
                    match key.and_then(|key| keys.iter().position(|&candidate| key == candidate)) {
                        Some(0) => 7,
                        Some(1) => 11,
                        Some(2) => 17,
                        _ => 23,
                    };
                arm.if_(&leave, |branch| branch.branch(&exit, (word, 1000u64)))?;
                arm.yield_((word, 2000u64))
            })?;
            block.yield_(result)
        })?;
        body.return_(wide.add(word.unsigned().extend::<I64>()))
    })
}

fn narrow_results() -> TestModule {
    Fixture::new().function(&[Type::I8, Type::I1], Some(Type::I32), |mut body| {
        let byte = body.parameter::<I8>(0)?;
        let bit = body.parameter::<I1>(1)?;
        let (byte, bit) = body.block::<(I8, I1)>(|mut block, exit| {
            block.if_(&bit, |branch| {
                branch.branch(&exit, (byte.add(1), bit.add(true)))
            })?;
            block.yield_((byte.add(2), bit.add(true)))
        })?;
        body.return_(
            byte.unsigned()
                .extend::<I32>()
                .or(bit.unsigned().extend::<I32>().shl(8)),
        )
    })
}

#[derive(Default)]
struct Code {
    result_shapes: Vec<Vec<ValType>>,
    loads: usize,
    calls: usize,
    local_writes: usize,
    fault_constants: usize,
}

fn inspect(module: &TestModule) -> Code {
    Validator::new().validate_all(module.bytes()).unwrap();
    let mut code = Code::default();
    for payload in Parser::new(0).parse_all(module.bytes()) {
        match payload.unwrap() {
            Payload::TypeSection(types) => {
                for ty in types.into_iter_err_on_gc_types() {
                    let results = ty.unwrap().results().to_vec();
                    if results.len() > 1 {
                        code.result_shapes.push(results);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                for op in body.get_operators_reader().unwrap() {
                    match op.unwrap() {
                        Operator::I32Load { .. } | Operator::I64Load { .. } => code.loads += 1,
                        Operator::Call { .. } => code.calls += 1,
                        Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                            code.local_writes += 1
                        }
                        Operator::I32Const { value: 0xf00d } => code.fault_constants += 1,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    code
}

#[test]
fn nested_outward_exits_share_one_failure_tail_and_preserve_prior_reads() {
    let module = shared_exits();
    let code = inspect(&module);
    assert_eq!(code.result_shapes, [vec![ValType::I32, ValType::I32]]);
    assert_eq!(code.fault_constants, 1);
    for (arguments, result, memory) in [
        ((0, 1, 0, 20), 227, [7, 0, 0, 0, 0, 0, 0, 0]),
        ((1, 0, 0, 20), 227, [7, 0, 0, 0, 0, 0, 0, 0]),
        ((1, 1, 0, 20), 127, [11, 0, 0, 0, 1, 0, 0, 0]),
        ((0, 1, 1, 20), 20, [7, 0, 0, 0, 0x0d, 0xf0, 0, 0]),
        (
            (1, 1, 1, 20),
            4_294_971_412,
            [11, 0, 0, 0, 0x0d, 0xf0, 0, 0],
        ),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i64>(arguments).unwrap(), result);
        assert_eq!(&instance.memory("state")[..8], memory);
    }
}

#[test]
fn unused_components_remove_trapping_reads_without_removing_selected_effects() {
    for observe in [true, false] {
        let module = projected_results(observe);
        let code = inspect(&module);
        assert_eq!(code.loads, 0);
        assert_eq!(code.calls, 1);
        assert_eq!(
            code.result_shapes,
            if observe {
                vec![vec![ValType::I32, ValType::I32]]
            } else {
                vec![]
            }
        );
        let mut instance = module.instantiate();
        assert_eq!(
            instance.call::<i32>(1).unwrap(),
            if observe { 6 } else { 17 }
        );
        assert_eq!(&instance.memory("state")[..2], &[1, 0x5a]);
        assert_eq!(
            instance.callbacks(),
            &[Call::new("receive", &[Value::I32(9)])
                .with_memories(&[MemoryBytes::new("state", &[1, 0x5a])])]
        );
        let mut instance = module.instantiate();
        assert_eq!(
            instance.call::<i32>(0).unwrap(),
            if observe { 9 } else { 17 }
        );
        assert_eq!(&instance.memory("state")[..2], &[0xa5, 0x5a]);
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn nested_mixed_results_forward_the_ordered_result_stack() {
    let module = nested_results();
    let code = inspect(&module);
    assert_eq!(
        code.result_shapes,
        [vec![ValType::I32, ValType::I32, ValType::I64]]
    );
    assert_eq!(code.local_writes, 3);
    assert_eq!(module.instantiate().call::<i64>(1).unwrap(), 2_147_483_648);
    assert_eq!(
        module.instantiate().call::<i64>(0).unwrap(),
        -9_223_372_036_854_775_801
    );
}

#[test]
fn a_dead_component_omits_its_pure_call_and_keeps_the_block_store() {
    let module = unused_pure_call();
    assert_eq!(inspect(&module).calls, 0);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 7);
    assert_eq!(&instance.memory("state")[..2], &[1, 0x5a]);
}

#[test]
fn reordered_duplicated_and_subset_results_preserve_their_component_values() {
    for (indices, expected) in [([1, 0], 131_073), ([0, 0], 65_537), ([2, 0], 196_609)] {
        let module = Fixture::new().function(&[Type::I1], Some(Type::I32), |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let (first, second) = body.block::<(I32, I32)>(|mut block, _| {
                let (a, b, c) = block.if_value::<(I32, I32, I32)>(
                    &condition,
                    |arm| arm.yield_((1, 2, 3)),
                    |arm| arm.yield_((11, 12, 13)),
                )?;
                let values = [a, b, c];
                block.yield_((&values[indices[0]], &values[indices[1]]))
            })?;
            body.return_(first.shl(16).or(second))
        });
        inspect(&module);
        assert_eq!(module.instantiate().call::<i32>(1).unwrap(), expected);
        assert_eq!(
            module.instantiate().call::<i32>(0).unwrap(),
            expected + 655_370
        );
    }
}

#[test]
fn outward_labels_cross_dense_and_sparse_switch_wrappers() {
    for keys in [[2, 3, 5], [0, 0x8000_0000, u32::MAX]] {
        let module = switch_exits(&keys);
        inspect(&module);
        for (selector, expected) in [(keys[0], 7), (keys[1], 11), (keys[2], 17), (19, 23)] {
            for leave in [0, 1] {
                assert_eq!(
                    module
                        .instantiate()
                        .call::<i64>((selector as i32, leave))
                        .unwrap(),
                    expected + if leave == 1 { 1000 } else { 2000 }
                );
            }
        }
    }
}

#[test]
fn every_narrow_result_component_normalizes_at_its_observer() {
    let module = narrow_results();
    inspect(&module);
    for (arguments, expected) in [
        ((255, 1), 0),
        ((255, 0), 257),
        ((127, 1), 128),
        ((1, 0), 259),
    ] {
        assert_eq!(
            module.instantiate().call::<i32>(arguments).unwrap(),
            expected
        );
    }
}

#[test]
fn unit_blocks_and_result_arms_accept_fallthrough() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0, 0, 0, 0]);
    let module = fixture.function(&[Type::I1], Some(Type::I32), |mut body| {
        let condition = body.parameter::<I1>(0)?;
        body.block::<()>(|mut block, exit| {
            block.if_(&condition, |branch| branch.branch(&exit, ()))?;
            block.if_value::<()>(true, |mut arm| arm.store::<I32>(state, 0, 7), |_| Ok(()))?;
            block.switch_value::<(), _>(condition.unsigned().extend::<I32>(), &[0], |mut arm, _| {
                arm.store::<I32>(state, 0, 11)
            })
        })?;
        let value = body.load::<I32>(state, 0)?;
        body.return_(value)
    });
    inspect(&module);
    assert_eq!(module.instantiate().call::<i32>(0).unwrap(), 11);
    assert_eq!(module.instantiate().call::<i32>(1).unwrap(), 0);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn typed_blocks_execute_shared_exits_and_result_projection_in_v8() {
    let module = shared_exits();
    for (arguments, result, memory) in [
        ([0, 1, 0, 20], 227, [7, 0, 0, 0, 0, 0, 0, 0]),
        ([1, 0, 0, 20], 227, [7, 0, 0, 0, 0, 0, 0, 0]),
        ([1, 1, 0, 20], 127, [11, 0, 0, 0, 1, 0, 0, 0]),
        ([0, 1, 1, 20], 20, [7, 0, 0, 0, 0x0d, 0xf0, 0, 0]),
        (
            [1, 1, 1, 20],
            4_294_971_412,
            [11, 0, 0, 0, 0x0d, 0xf0, 0, 0],
        ),
    ] {
        let input =
            Input::call("run", &arguments.map(Value::I32)).with_memories(&[MemoryBytes::new(
                "state",
                &[7, 0, 0, 0, 0xa5, 0xa5, 0xa5, 0xa5],
            )]);
        assert_eq!(
            module.run_v8(&input),
            Observation::returned(Value::I64(result))
                .with_memories(&[MemoryBytes::new("state", &memory)])
        );
    }
    for observe in [true, false] {
        let module = projected_results(observe);
        for condition in [0, 1] {
            let input = Input::call("run", &[Value::I32(condition)])
                .with_memories(&[MemoryBytes::new("state", &[0xa5, 0x5a])])
                .with_callbacks(&[Callback::new("receive", Value::I32(23))]);
            let result = if !observe {
                17
            } else if condition == 0 {
                9
            } else {
                6
            };
            let memory = if condition == 0 {
                [0xa5, 0x5a]
            } else {
                [1, 0x5a]
            };
            let mut expected = Observation::returned(Value::I32(result))
                .with_memories(&[MemoryBytes::new("state", &memory)]);
            if condition != 0 {
                expected = expected.with_callbacks(&[Call::new("receive", &[Value::I32(9)])
                    .with_memories(&[MemoryBytes::new("state", &[1, 0x5a])])]);
            }
            assert_eq!(module.run_v8(&input), expected);
        }
    }
    let module = unused_pure_call();
    assert_eq!(
        module.run_v8(
            &Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", &[0xa5, 0x5a])])
        ),
        Observation::returned(Value::I32(7))
            .with_memories(&[MemoryBytes::new("state", &[1, 0x5a])])
    );
    let module = nested_results();
    for (condition, result) in [(1, 2_147_483_648), (0, -9_223_372_036_854_775_801)] {
        assert_eq!(
            module.run_v8(&Input::call("run", &[Value::I32(condition)])),
            Observation::returned(Value::I64(result))
        );
    }
    for keys in [[2, 3, 5], [0, 0x8000_0000, u32::MAX]] {
        let module = switch_exits(&keys);
        for (selector, result) in [(keys[1], 1011), (19, 1023)] {
            assert_eq!(
                module.run_v8(&Input::call(
                    "run",
                    &[Value::I32(selector as i32), Value::I32(1)]
                )),
                Observation::returned(Value::I64(result))
            );
        }
    }
}
