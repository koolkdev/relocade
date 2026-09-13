use super::entry_operators;
use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{Type, I1, I32, I8};

fn different_tail_arguments(trap_on_taken: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let trapped_value = fixture
        .program
        .function(signature(&[], &[Type::I32]), |body| body.trap())
        .unwrap();
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        let result = body.block::<I32>(|mut outer, exit| {
            let result = outer.block::<I32>(|mut inner, _| {
                let trapped = inner.call::<I32>(trapped_value, &[])?;
                let safe = inner.value::<I32>(7)?;
                let (taken, otherwise) = if trap_on_taken {
                    (trapped, safe)
                } else {
                    (safe, trapped)
                };
                inner.branch_if(choose, &exit, taken)?;
                inner.yield_(otherwise)
            })?;
            outer.yield_(result.add(100))
        })?;
        body.return_(result)
    })
}

#[test]
fn conditional_edges_evaluate_only_their_selected_arguments() {
    for trap_on_taken in [false, true] {
        let module = different_tail_arguments(trap_on_taken);
        entry_operators(&module);
        for choose in [0, 1] {
            let expected = if (choose != 0) == trap_on_taken {
                Err(wasmtime::Trap::UnreachableCodeReached)
            } else {
                Ok(if choose != 0 { 7 } else { 107 })
            };
            assert_eq!(module.instantiate().call::<i32>(choose), expected);
        }
    }
}

fn unused_exit_argument() -> TestModule {
    let mut fixture = Fixture::new();
    let trapped_value = fixture
        .program
        .function(signature(&[], &[Type::I32]), |body| body.trap())
        .unwrap();
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let repeat = body.parameter::<I1>(0)?;
        let result = body.loop_::<(I32, I32), (I32, I32)>((0, 0), |mut iteration, labels, _| {
            let trapped = iteration.call::<I32>(trapped_value, &[])?;
            let next = (trapped, 7);
            iteration.branch_if(repeat, &labels.again, next.clone())?;
            iteration.yield_(next)
        })?;
        body.return_(result.1)
    })
}

#[test]
fn an_argument_dead_on_exit_remains_lazy_even_when_the_backedge_needs_it() {
    let module = unused_exit_argument();
    entry_operators(&module);
    assert_eq!(module.instantiate().call::<i32>(0), Ok(7));
    assert_eq!(
        module.instantiate().call::<i32>(1),
        Err(wasmtime::Trap::UnreachableCodeReached)
    );
}

#[test]
fn both_conditional_edges_contribute_to_the_same_outward_result() {
    let module = Fixture::new().function(&[Type::I1], &[Type::I8], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        let result = body.block::<I8>(|mut outer, exit| {
            outer.block::<()>(|mut inner, _| {
                inner.branch_if(choose, &exit, 7)?;
                inner.branch(&exit, 255)
            })?;
            outer.trap()
        })?;
        body.return_(result.add(1))
    });
    entry_operators(&module);
    assert_eq!(module.instantiate().call::<i32>(1), Ok(8));
    assert_eq!(module.instantiate().call::<i32>(0), Ok(0));
}

fn tail_writes() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let write = fixture
        .program
        .function(signature(&[Type::I32], &[Type::I32]), |mut body| {
            let value = body.parameter::<I32>(0)?;
            body.if_(value.ne(0), |mut arm| {
                arm.store(state, 0, &value)?;
                arm.return_(23)
            })?;
            body.return_(0)
        })
        .unwrap();
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let before = body.load::<I32>(state, 0)?;
        let result = body.loop_::<(I32, I32), (I32, I32)>(
            (count, 0),
            |mut iteration, labels, (left, sum)| {
                let fresh = iteration.load::<I32>(state, 0)?;
                let next = (left.sub(1), sum.add(&before).add(fresh));
                iteration.if_(next.0.ne(0), |mut arm| {
                    // The unused result must not remove this helper's tail write.
                    arm.call::<I32>(write, &[100.into()])?;
                    arm.branch(&labels.again, next.clone())
                })?;
                iteration.yield_(next)
            },
        )?;
        body.return_(result.1)
    })
}

#[test]
fn effectful_if_arms_preserve_snapshots_across_loop_backedges() {
    let module = tail_writes();
    entry_operators(&module);
    for (count, sum, stored) in [(1, 14, 7), (2, 121, 100), (3, 228, 100)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(count), Ok(sum));
        assert_eq!(&instance.memory("state")[..4], &[stored, 0, 0, 0]);
    }
}

fn conditional_switch_case(keys: &[u32]) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0, 0, 0, 0]);
    fixture.function(&[Type::I32, Type::I1], &[Type::I32], |mut body| {
        let selector = body.parameter::<I32>(0)?;
        let leave = body.parameter::<I1>(1)?;
        let result = body.block::<I32>(|mut outer, exit| {
            let result = outer.switch_value::<I32, _>(&selector, keys, |mut case, key| {
                if key == Some(keys[0]) {
                    case.branch_if(&leave, &exit, 7)?;
                    case.yield_(7)
                } else if key.is_some() {
                    case.yield_if(&leave, 19)?;
                    case.store::<I32>(state, 0, 99)?;
                    case.yield_(23)
                } else {
                    case.yield_(41)
                }
            })?;
            outer.yield_(result.add(100))
        })?;
        body.return_(result)
    })
}

#[test]
fn a_conditional_switch_case_leaves_the_case_table_on_both_edges() {
    for keys in [[2, 3], [2, 0x8000_0000]] {
        let module = conditional_switch_case(&keys);
        entry_operators(&module);
        for (selector, leave, result, stored) in [
            (keys[0], 0, 107, 0),
            (keys[0], 1, 7, 0),
            (keys[1], 0, 123, 99),
            (keys[1], 1, 119, 0),
            (99, 0, 141, 0),
        ] {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>((selector as i32, leave)), Ok(result));
            assert_eq!(&instance.memory("state")[..4], &[stored, 0, 0, 0]);
        }
    }
}

#[test]
fn an_unused_call_keeps_its_conditional_tail_call_effects() {
    let mut fixture = Fixture::new();
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I32]),
        &[Value::I32(29)],
    );
    let helper = fixture
        .program
        .function(signature(&[Type::I1], &[Type::I32]), |mut body| {
            let choose = body.parameter::<I1>(0)?;
            body.if_(choose, |arm| arm.tail_call(receive, &[11.into()]))?;
            body.return_(7)
        })
        .unwrap();
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        body.call::<I32>(helper, &[choose.argument()])?;
        body.return_(17)
    });
    entry_operators(&module);
    for choose in [0, 1] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(choose), Ok(17));
        if choose == 0 {
            assert!(instance.callbacks().is_empty());
        } else {
            assert_eq!(
                instance.callbacks(),
                &[Call::new("receive", &[Value::I32(11)])]
            );
        }
    }
}

fn conditional_result(inverted: bool, store_offset: u32) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        let condition = if inverted { choose.eq(false) } else { choose };
        let result = body.block::<I32>(|mut block, _| {
            let before = block.load::<I32>(state, 4)?;
            block.store::<I32>(state, store_offset, 1)?;
            block.yield_if(condition, before)?;
            block.store::<I32>(state, 4, 9)?;
            let after = block.load::<I32>(state, 4)?;
            block.yield_(after.add(100))
        })?;
        body.return_(result)
    })
}

#[test]
fn conditional_yields_preserve_snapshots_and_leave_the_false_path_open() {
    for inverted in [false, true] {
        for store_offset in [0, 4] {
            let module = conditional_result(inverted, store_offset);
            entry_operators(&module);
            for choose in [0, 1] {
                let taken = (choose != 0) != inverted;
                let mut instance = module.instantiate();
                assert_eq!(
                    instance.call::<i32>(choose),
                    Ok(if taken { 11 } else { 109 })
                );
                let first = if store_offset == 0 { 1 } else { 7 };
                let second = if !taken {
                    9
                } else if store_offset == 4 {
                    1
                } else {
                    11
                };
                assert_eq!(
                    &instance.memory("state")[..10],
                    &[first, 0, 0, 0, second, 0, 0, 0, 0xa5, 0x5a]
                );
            }
        }
    }
}

fn dead_conditional_result() -> TestModule {
    let mut fixture = Fixture::new();
    let trapped_value = fixture
        .program
        .function(signature(&[], &[Type::I32]), |body| body.trap())
        .unwrap();
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        let result = body.block::<(I32, I32)>(|mut block, _| {
            let trapped = block.call::<I32>(trapped_value, &[])?;
            block.yield_if(choose, (trapped, 7))?;
            block.yield_((0, 11))
        })?;
        body.return_(result.1)
    })
}

#[test]
fn a_taken_conditional_yield_omits_its_dead_result_components() {
    let module = dead_conditional_result();
    entry_operators(&module);
    for (choose, expected) in [(0, 11), (1, 7)] {
        assert_eq!(module.instantiate().call::<i32>(choose), Ok(expected));
    }
}

fn reversed_backedge() -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I32, Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let result = body.loop_::<(I32, I32), (I32, I32)>(
            (count, 0),
            |mut iteration, labels, (left, sum)| {
                let next = (left.sub(1), sum.add(3));
                iteration.yield_if(next.0.eq(0), next.clone())?;
                iteration.branch(&labels.again, next)
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn a_conditional_loop_result_shares_the_backedge_tuple_with_reversed_polarity() {
    let module = reversed_backedge();
    let operators = entry_operators(&module);
    assert_eq!(
        operators
            .iter()
            .filter(|op| matches!(op, wasmparser::Operator::BrIf { .. }))
            .count(),
        1
    );
    assert!(!operators
        .iter()
        .any(|op| matches!(op, wasmparser::Operator::If { .. })));
    assert!(operators
        .iter()
        .any(|op| matches!(op, wasmparser::Operator::BrIf { relative_depth: 0 })));
    assert!(!operators
        .iter()
        .any(|op| matches!(op, wasmparser::Operator::Br { .. })));
    for (count, sum) in [(1, 3), (3, 9)] {
        assert_eq!(module.instantiate().call::<(i32, i32)>(count), Ok((0, sum)));
    }
}

#[test]
fn conditional_yields_use_the_direct_result_arm_destination() {
    let module = Fixture::new().function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let outer = body.parameter::<I1>(0)?;
        let early = body.parameter::<I1>(1)?;
        let result = body.if_value::<I32>(
            outer,
            |mut arm| {
                arm.yield_if(early, 7)?;
                arm.yield_(11)
            },
            |arm| arm.yield_(23),
        )?;
        body.return_(result.add(100))
    });
    entry_operators(&module);
    for (outer, early, expected) in [(0, 0, 123), (0, 1, 123), (1, 0, 111), (1, 1, 107)] {
        assert_eq!(
            module.instantiate().call::<i32>((outer, early)),
            Ok(expected)
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn conditional_exit_edges_execute_in_v8() {
    for (choose, expected) in [(0, 11), (1, 7)] {
        assert_eq!(
            dead_conditional_result().run_v8(&Input::call("run", &[Value::I32(choose)])),
            Observation::returned(&[Value::I32(expected)])
        );
    }
    assert_eq!(
        reversed_backedge().run_v8(&Input::call("run", &[Value::I32(3)])),
        Observation::returned(&[Value::I32(0), Value::I32(9)])
    );
    assert_eq!(
        conditional_result(false, 4).run_v8(&Input::call("run", &[Value::I32(0)]).with_memories(
            &[MemoryBytes::new(
                "state",
                &[7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]
            )]
        )),
        Observation::returned(&[Value::I32(109)]).with_memories(&[MemoryBytes::new(
            "state",
            &[7, 0, 0, 0, 9, 0, 0, 0, 0xa5, 0x5a]
        )])
    );
    for (trap_on_taken, choose, expected) in [(false, 1, 7), (true, 0, 107)] {
        assert_eq!(
            different_tail_arguments(trap_on_taken)
                .run_v8(&Input::call("run", &[Value::I32(choose)])),
            Observation::returned(&[Value::I32(expected)])
        );
    }
    assert_eq!(
        unused_exit_argument().run_v8(&Input::call("run", &[Value::I32(0)])),
        Observation::returned(&[Value::I32(7)])
    );
    assert_eq!(
        tail_writes().run_v8(
            &Input::call("run", &[Value::I32(2)])
                .with_memories(&[MemoryBytes::new("state", &[7, 0, 0, 0])])
        ),
        Observation::returned(&[Value::I32(121)])
            .with_memories(&[MemoryBytes::new("state", &[100, 0, 0, 0])])
    );
    for keys in [[2, 3], [2, 0x8000_0000]] {
        assert_eq!(
            conditional_switch_case(&keys).run_v8(
                &Input::call("run", &[Value::I32(2), Value::I32(0)])
                    .with_memories(&[MemoryBytes::new("state", &[0, 0, 0, 0])])
            ),
            Observation::returned(&[Value::I32(107)])
                .with_memories(&[MemoryBytes::new("state", &[0, 0, 0, 0])])
        );
    }
}
