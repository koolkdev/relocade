use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{BuildError, Type, I1, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn swaps() -> TestModule {
    Fixture::new().function(
        &[Type::I32],
        &[Type::I64, Type::I32, Type::I32],
        |mut body| {
            let count = body.parameter::<I32>(0)?;
            let result = body.loop_::<(I32, I32, I32, I64), (I64, I32, I32)>(
                (&count, 3, 7, 100u64),
                |mut iteration, labels, (left, first, second, sum)| {
                    iteration.if_(left.eq(0), |done| {
                        done.branch(&labels.exit, (&sum, &first, &second))
                    })?;
                    iteration.branch(
                        &labels.again,
                        (
                            left.sub(1),
                            &second,
                            &first,
                            sum.add(first.unsigned().extend::<I64>()),
                        ),
                    )
                },
            )?;
            body.return_(result)
        },
    )
}

fn nested_labels() -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let result =
            body.loop_::<(I32, I32), I32>((&count, 0), |mut outer, labels, (left, sum)| {
                outer.if_(left.eq(0), |done| done.branch(&labels.exit, &sum))?;
                let inner_result =
                    outer.loop_::<I32, I32>(0, |mut inner, inner_labels, index| {
                        inner.block::<()>(|mut nested, _| {
                            nested
                                .if_(left.eq(2), |done| done.branch(&labels.exit, sum.add(100)))?;
                            nested.if_(index.eq(1).and(left.unsigned().ge(3)), |next| {
                                next.branch(&labels.again, (left.sub(1), sum.add(10)))
                            })
                        })?;
                        inner.if_(index.eq(2), |done| done.branch(&inner_labels.exit, &index))?;
                        inner.branch(&inner_labels.again, index.add(1))
                    })?;
                outer.branch(&labels.again, (left.sub(1), sum.add(inner_result)))
            })?;
        body.return_(result)
    })
}

fn snapshots() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let before = body.load::<I32>(state, 0)?;
        let sum =
            body.loop_::<(I32, I32), I32>((&count, 0), |mut iteration, labels, (left, sum)| {
                iteration.if_(left.eq(0), |done| done.branch(&labels.exit, &sum))?;
                let fresh = iteration.load::<I32>(state, 0)?;
                iteration.store(state, 0, fresh.add(1))?;
                iteration.branch(&labels.again, (left.sub(1), sum.add(fresh)))
            })?;
        body.return_(sum.add(before))
    })
}

fn entry_reads_used_on_backedges() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a, 0, 0]);
    let read_only = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut body| {
            let value = body.load::<I32>(state, 4)?;
            body.return_(value)
        })
        .unwrap();
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let before = body.load::<I32>(state, 0)?;
        let called_before = body.call::<I32>(read_only, &[])?;
        let sum =
            body.loop_::<(I32, I32), I32>((&count, 0), |mut iteration, labels, (left, sum)| {
                iteration.if_(left.eq(0), |done| done.branch(&labels.exit, &sum))?;
                let fresh = iteration.load::<I32>(state, 0)?;
                let fresh_call_source = iteration.load::<I32>(state, 4)?;
                let next_sum = sum.add(&before).add(&called_before).add(&fresh);
                // Both entry snapshots are used before the writes in lexical order.
                // A backedge must still preserve them for the next iteration.
                iteration.store(state, 8, &next_sum)?;
                iteration.store(state, 0, fresh.add(1))?;
                iteration.store(state, 4, fresh_call_source.add(10))?;
                iteration.branch(&labels.again, (left.sub(1), next_sum))
            })?;
        body.return_(sum)
    })
}

fn narrow_values() -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I32, Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let result = body.loop_::<(I32, I8, I1, I32), (I32, I32)>(
            (&count, 254, true, 0),
            |mut iteration, labels, (left, byte, bit, hits)| {
                iteration.if_(left.eq(0), |done| {
                    done.branch(
                        &labels.exit,
                        (
                            byte.unsigned()
                                .extend::<I32>()
                                .or(bit.unsigned().extend::<I32>().shl(8)),
                            &hits,
                        ),
                    )
                })?;
                iteration.branch(
                    &labels.again,
                    (
                        left.sub(1),
                        byte.add(1),
                        bit.add(true),
                        hits.add(byte.unsigned().lt(1).unsigned().extend::<I32>()),
                    ),
                )
            },
        )?;
        body.return_(result)
    })
}

fn direct_and_unit_exits() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0, 0, 0, 0]);
    fixture.function(&[], &[Type::I32], |mut body| {
        body.loop_::<(), ()>((), |mut iteration, _, ()| {
            iteration.store::<I32>(state, 0, 9)
        })?;
        let value = body.loop_::<(I32, I64), I32>((7, 11u64), |iteration, _, (word, wide)| {
            iteration.yield_(word.add(wide.truncate::<I32>()))
        })?;
        body.return_(value)
    })
}

fn validate(module: &TestModule) {
    Validator::new().validate_all(module.bytes()).unwrap();
}

#[test]
fn multivalue_backedges_swap_simultaneously_and_keep_distinct_exit_shape() {
    let module = swaps();
    validate(&module);
    assert!(Parser::new(0).parse_all(module.bytes()).any(|payload| {
        matches!(payload.unwrap(), Payload::CodeSectionEntry(body)
            if body.get_operators_reader().unwrap().into_iter().any(|op| matches!(op.unwrap(), Operator::Loop { .. })))
    }));
    for (count, expected) in [
        (0, (100, 3, 7)),
        (1, (103, 7, 3)),
        (2, (110, 3, 7)),
        (5, (123, 7, 3)),
    ] {
        assert_eq!(
            module.instantiate().call::<(i64, i32, i32)>(count).unwrap(),
            expected
        );
    }
}

#[test]
fn nested_continue_and_exit_labels_preserve_outer_iteration_values() {
    let module = nested_labels();
    validate(&module);
    for (count, expected) in [(0, 0), (1, 2), (2, 100), (3, 110), (4, 120)] {
        assert_eq!(module.instantiate().call::<i32>(count).unwrap(), expected);
    }
}

#[test]
fn preloop_snapshot_survives_stores_while_loop_loads_refresh_each_iteration() {
    let module = snapshots();
    validate(&module);
    for (count, expected, stored) in [(0, 7, 7), (1, 14, 8), (3, 31, 10)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(count).unwrap(), expected);
        assert_eq!(
            &instance.memory("state")[..6],
            &[stored, 0, 0, 0, 0xa5, 0x5a]
        );
    }
}

#[test]
fn entry_load_and_read_only_call_remain_snapshots_across_later_iteration_stores() {
    let module = entry_reads_used_on_backedges();
    validate(&module);
    for (count, result, memory) in [
        (0, 0, [7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a, 0, 0]),
        (1, 25, [8, 0, 0, 0, 21, 0, 0, 0, 25, 0, 0, 0]),
        (3, 78, [10, 0, 0, 0, 41, 0, 0, 0, 78, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(count).unwrap(), result);
        assert_eq!(&instance.memory("state")[..12], &memory);
    }
}

#[test]
fn narrow_carried_values_wrap_at_logical_observers_across_backedges() {
    let module = narrow_values();
    validate(&module);
    for (count, expected) in [
        (0, (510, 0)),
        (1, (255, 0)),
        (2, (256, 0)),
        (3, (1, 1)),
        (4, (258, 1)),
    ] {
        assert_eq!(
            module.instantiate().call::<(i32, i32)>(count).unwrap(),
            expected
        );
    }
}

#[test]
fn direct_yield_and_unit_fallthrough_complete_once() {
    let module = direct_and_unit_exits();
    validate(&module);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 18);
    assert_eq!(&instance.memory("state")[..4], &[9, 0, 0, 0]);
}

#[test]
fn loop_inputs_and_labels_cannot_escape_and_failed_builds_leave_parent_usable() {
    let mut fixture = Fixture::new();
    let function = fixture.program.declare(signature(&[], &[Type::I32]));
    let mut body = fixture.program.define(function).unwrap();
    let mut escaped_value = None;
    let mut escaped_again = None;
    let mut escaped_exit = None;
    let result = body
        .loop_::<I32, I32>(7, |iteration, labels, value| {
            escaped_value = Some(value.clone());
            escaped_again = Some(labels.again.clone());
            escaped_exit = Some(labels.exit.clone());
            iteration.yield_(value)
        })
        .unwrap();
    assert_eq!(
        body.value(escaped_value.unwrap()).err(),
        Some(BuildError::OutOfScope)
    );
    for label in [
        escaped_again.as_ref().unwrap(),
        escaped_exit.as_ref().unwrap(),
    ] {
        assert_eq!(
            body.if_(true, |branch| branch.branch(label, 9)),
            Err(BuildError::OutOfScope)
        );
    }
    assert_eq!(
        body.loop_::<(I32, I1), I32>(7, |iteration, _, _| iteration.yield_(0))
            .err(),
        Some(BuildError::ResultCount {
            expected: 2,
            actual: 1
        })
    );
    assert_eq!(
        body.loop_::<I32, I32>(true, |iteration, _, _| iteration.yield_(0))
            .err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I32,
            actual: Type::I1
        })
    );
    assert_eq!(
        body.loop_::<I32, I32>(1, |iteration, labels, _| iteration
            .branch(&labels.again, true))
            .err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I32,
            actual: Type::I1
        })
    );
    body.return_(result).unwrap();
    validate(&fixture.finish(function));
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn typed_loops_execute_in_v8() {
    let module = entry_reads_used_on_backedges();
    assert_eq!(
        module.run_v8(
            &Input::call("run", &[Value::I32(3)]).with_memories(&[MemoryBytes::new(
                "state",
                &[7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a, 0, 0]
            )])
        ),
        Observation::returned(&[Value::I32(78)]).with_memories(&[MemoryBytes::new(
            "state",
            &[10, 0, 0, 0, 41, 0, 0, 0, 78, 0, 0, 0]
        )])
    );
    let module = swaps();
    for (count, expected) in [
        (0, [Value::I64(100), Value::I32(3), Value::I32(7)]),
        (5, [Value::I64(123), Value::I32(7), Value::I32(3)]),
    ] {
        assert_eq!(
            module.run_v8(&Input::call("run", &[Value::I32(count)])),
            Observation::returned(&expected)
        );
    }
    let module = nested_labels();
    for (count, expected) in [(1, 2), (4, 120)] {
        assert_eq!(
            module.run_v8(&Input::call("run", &[Value::I32(count)])),
            Observation::returned(&[Value::I32(expected)])
        );
    }
    let module = snapshots();
    assert_eq!(
        module.run_v8(
            &Input::call("run", &[Value::I32(3)])
                .with_memories(&[MemoryBytes::new("state", &[7, 0, 0, 0, 0xa5, 0x5a])])
        ),
        Observation::returned(&[Value::I32(31)])
            .with_memories(&[MemoryBytes::new("state", &[10, 0, 0, 0, 0xa5, 0x5a])])
    );
    let module = narrow_values();
    assert_eq!(
        module.run_v8(&Input::call("run", &[Value::I32(3)])),
        Observation::returned(&[Value::I32(1), Value::I32(1)])
    );
}

fn guarded_invariant_reads() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 11, 0, 0, 0, 2, 0, 0, 0]);
    let read_only = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut helper| {
            let value = helper.load::<I32>(state, 4)?;
            helper.return_(value)
        })
        .unwrap();
    fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let enabled = body.parameter::<I1>(0)?;
            let allowed = body.parameter::<I1>(1)?;
            let count = body.parameter::<I32>(2)?;
            let result = body.if_value::<I32>(
                &enabled,
                |mut outer| {
                    let result = outer.if_value::<I32>(
                        &allowed,
                        |mut guarded| {
                            let snapshot = guarded.load::<I32>(state, 0)?;
                            let called = guarded.call::<I32>(read_only, &[])?;
                            let result = guarded.loop_::<(I32, I32), I32>(
                                (&count, 0),
                                |mut iteration, labels, (left, sum)| {
                                    iteration
                                        .if_(left.eq(0), |done| done.branch(&labels.exit, &sum))?;
                                    let fresh = iteration.load::<I32>(state, 8)?;
                                    iteration.store(state, 8, fresh.add(1))?;
                                    iteration.branch(
                                        &labels.again,
                                        (left.sub(1), sum.add(&snapshot).add(&called).add(fresh)),
                                    )
                                },
                            )?;
                            guarded.yield_(result)
                        },
                        |disabled| disabled.yield_(11),
                    )?;
                    outer.yield_(result)
                },
                |disabled| disabled.yield_(17),
            )?;
            body.return_(result)
        },
    )
}

#[test]
fn guarded_entry_snapshots_stay_outside_loop_and_fresh_reads_stay_inside() {
    use wasmparser::{ExternalKind, TypeRef};
    let module = guarded_invariant_reads();
    validate(&module);
    let mut imported = 0;
    let mut defined = 0;
    let mut entry = None;
    for payload in Parser::new(0).parse_all(module.bytes()) {
        match payload.unwrap() {
            Payload::ImportSection(section) => {
                for import in section {
                    if matches!(import.unwrap().ty, TypeRef::Func(_)) {
                        imported += 1;
                    }
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.name == "run" && export.kind == ExternalKind::Func {
                        entry = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let function = imported + defined;
                defined += 1;
                if Some(function) != entry {
                    continue;
                }
                // Track semantic control ancestry, without constraining local IDs,
                // result types, instruction counts, or incidental block wrappers.
                let mut control = Vec::<(u8, usize, bool)>::new();
                let mut invariant_reads = Vec::new();
                let mut fresh_reads = Vec::new();
                let mut loops = Vec::new();
                let mut saw_load = false;
                let mut saw_call = false;
                for (position, operator) in
                    body.get_operators_reader().unwrap().into_iter().enumerate()
                {
                    let guards = control
                        .iter()
                        .filter(|(kind, _, _)| *kind == 1)
                        .map(|(_, id, arm)| (*id, *arm))
                        .collect::<Vec<_>>();
                    let in_loop = control.iter().any(|(kind, _, _)| *kind == 2);
                    match operator.unwrap() {
                        Operator::If { .. } => control.push((1, position, true)),
                        Operator::Block { .. } => control.push((0, position, true)),
                        Operator::Loop { .. } => {
                            loops.push((position, guards));
                            control.push((2, position, true));
                        }
                        Operator::Else => {
                            control.last_mut().unwrap().2 = false;
                        }
                        Operator::End => {
                            control.pop();
                        }
                        Operator::I32Load { memarg } if memarg.offset == 0 => {
                            saw_load = true;
                            invariant_reads.push((position, guards, in_loop));
                        }
                        Operator::Call { .. } => {
                            saw_call = true;
                            invariant_reads.push((position, guards, in_loop));
                        }
                        Operator::I32Load { memarg } if memarg.offset == 8 => {
                            fresh_reads.push((guards, in_loop))
                        }
                        _ => {}
                    }
                }
                let (loop_position, loop_guards) =
                    loops.first().expect("the counted loop remains structured");
                assert!(
                    loop_guards.len() >= 2,
                    "both authored guards must surround the loop"
                );
                assert!(
                    saw_load && saw_call,
                    "the entry load and read-only call remain demanded"
                );
                for (position, guards, in_loop) in invariant_reads {
                    assert!(
                        !in_loop && position < *loop_position,
                        "entry snapshots must not run in a loop header"
                    );
                    assert_eq!(
                        &guards, loop_guards,
                        "capture must stay within its authored guards"
                    );
                }
                assert!(!fresh_reads.is_empty());
                for (guards, in_loop) in fresh_reads {
                    assert!(
                        in_loop && guards.starts_with(loop_guards),
                        "body reads remain inside the guarded loop"
                    );
                }
            }
            _ => {}
        }
    }
    for (arguments, result, last) in [
        ((0, 1, 2), 17, 2),
        ((1, 0, 2), 11, 2),
        ((1, 1, 0), 0, 2),
        ((1, 1, 2), 41, 4),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(arguments).unwrap(), result);
        assert_eq!(
            &instance.memory("state")[..12],
            &[7, 0, 0, 0, 11, 0, 0, 0, last, 0, 0, 0]
        );
    }
}
