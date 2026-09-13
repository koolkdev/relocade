use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, Observation, TestModule, Value};
use wasm86_compiler::{Type, I1, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

#[path = "conditional_branches/exits.rs"]
mod exits;

fn entry_operators(module: &TestModule) -> Vec<Operator<'_>> {
    Validator::new().validate_all(module.bytes()).unwrap();
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
                    if export.name == "run" {
                        entry = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let index = imported + defined;
                defined += 1;
                if Some(index) == entry {
                    return body
                        .get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .map(Result::unwrap)
                        .collect();
                }
            }
            _ => {}
        }
    }
    panic!("the run export has a body");
}

fn swapped_tail() -> TestModule {
    Fixture::new().function(
        &[Type::I32],
        &[Type::I32, Type::I32, Type::I32, Type::I8, Type::I64],
        |mut body| {
            let count = body.parameter::<I32>(0)?;
            let result = body.loop_::<(I32, I32, I32, I8, I64), (I32, I32, I32, I8, I64)>(
                (count, 3, 7, 254, 100u64),
                |mut iteration, labels, (left, first, second, byte, sum)| {
                    let next = (
                        left.sub(1),
                        second,
                        first.clone(),
                        byte.add(1),
                        sum.add(first.unsigned().extend::<I64>()),
                    );
                    iteration.branch_if(next.0.ne(0), &labels.again, next.clone())?;
                    iteration.yield_(next)
                },
            )?;
            body.return_(result)
        },
    )
}

#[test]
fn a_shared_tail_tuple_uses_br_if_and_swaps_all_channels_simultaneously() {
    let module = swapped_tail();
    let operators = entry_operators(&module);
    assert_eq!(
        operators
            .iter()
            .filter(|op| matches!(op, Operator::BrIf { .. }))
            .count(),
        1
    );
    assert!(!operators.iter().any(|op| matches!(op, Operator::If { .. })));
    for (count, expected) in [
        (1, (0, 7, 3, 255, 103)),
        (2, (0, 3, 7, 0, 110)),
        (5, (0, 7, 3, 3, 123)),
    ] {
        assert_eq!(
            module
                .instantiate()
                .call::<(i32, i32, i32, i32, i64)>(count)
                .unwrap(),
            expected
        );
    }
}

fn nested_unit_tail() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0, 0, 0, 0]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let leave = body.parameter::<I1>(0)?;
        body.block::<()>(|mut outer, exit| {
            outer.block::<()>(|mut inner, _| {
                inner.branch_if(leave, &exit, ())?;
                inner.store::<I32>(state, 0, 3)?;
                inner.yield_(())
            })?;
            outer.store::<I32>(state, 0, 9)
        })?;
        let result = body.load::<I32>(state, 0)?;
        body.return_(result)
    })
}

#[test]
fn a_conditional_unit_exit_keeps_the_outward_branch_depth() {
    let module = nested_unit_tail();
    assert!(entry_operators(&module)
        .iter()
        .any(|op| matches!(op, Operator::BrIf { relative_depth: 1 })));
    assert_eq!(module.instantiate().call::<i32>(0).unwrap(), 9);
    assert_eq!(module.instantiate().call::<i32>(1).unwrap(), 0);
}

fn snapshot_tail() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 11, 0, 0, 0]);
    let read_only = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut body| {
            let result = body.load::<I32>(state, 4)?;
            body.return_(result)
        })
        .unwrap();
    fixture.function(&[Type::I32], &[Type::I32, Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let before = body.load::<I32>(state, 0)?;
        let called = body.call::<I32>(read_only, &[])?;
        let result = body.loop_::<(I32, I32), (I32, I32)>(
            (count, 0),
            |mut iteration, labels, (left, sum)| {
                let fresh = iteration.load::<I32>(state, 0)?;
                iteration.store(state, 0, fresh.add(1))?;
                let next = (left.sub(1), sum.add(&before).add(&called).add(fresh));
                iteration.branch_if(next.0.ne(0), &labels.again, next.clone())?;
                iteration.yield_(next)
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn shared_tail_demands_preserve_entry_snapshots_and_refresh_iteration_loads() {
    let module = snapshot_tail();
    let operators = entry_operators(&module);
    assert!(operators
        .iter()
        .any(|op| matches!(op, Operator::BrIf { .. })));
    let header = operators
        .iter()
        .position(|op| matches!(op, Operator::Loop { .. }))
        .unwrap();
    assert_eq!(
        operators
            .iter()
            .filter(|op| matches!(op, Operator::Call { .. }))
            .count(),
        1
    );
    assert!(operators[..header]
        .iter()
        .any(|op| matches!(op, Operator::Call { .. })));
    for (count, expected, stored) in [(1, 25, 8), (3, 78, 10)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<(i32, i32)>(count).unwrap(), (0, expected));
        assert_eq!(
            &instance.memory("state")[..8],
            &[stored, 0, 0, 0, 11, 0, 0, 0]
        );
    }
}

#[derive(Clone, Copy)]
enum Boundary {
    DifferentTuple,
    ArmStore,
    LoadCondition,
    CallCondition,
}

fn ordinary_tail(boundary: Boundary) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[1, 0, 0, 0, 0, 0, 0, 0]);
    let read_only = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut body| {
            let result = body.load::<I32>(state, 0)?;
            body.return_(result)
        })
        .unwrap();
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let result = body.block::<I32>(|mut outer, exit| {
            let result = outer.block::<I32>(|mut inner, _| {
                let condition = match boundary {
                    Boundary::LoadCondition => inner.load::<I32>(state, 0)?.ne(0),
                    Boundary::CallCondition => inner.call::<I32>(read_only, &[])?.ne(0),
                    _ => input.ne(0),
                };
                // A condition snapshot must survive a write before its consumer.
                inner.store::<I32>(state, 0, 0)?;
                let outgoing = input.add(7);
                if matches!(boundary, Boundary::ArmStore) {
                    inner.if_(condition, |mut arm| {
                        arm.store::<I32>(state, 4, 23)?;
                        arm.branch(&exit, &outgoing)
                    })?;
                } else {
                    inner.branch_if(condition, &exit, &outgoing)?;
                }
                inner.yield_(if matches!(boundary, Boundary::DifferentTuple) {
                    input.add(9)
                } else {
                    outgoing
                })
            })?;
            outer.yield_(result.add(100))
        })?;
        body.return_(result)
    })
}

#[test]
fn different_tuples_and_effectful_arms_keep_ordinary_if() {
    for boundary in [Boundary::DifferentTuple, Boundary::ArmStore] {
        let module = ordinary_tail(boundary);
        let operators = entry_operators(&module);
        assert!(operators.iter().any(|op| matches!(op, Operator::If { .. })));
        assert!(!operators
            .iter()
            .any(|op| matches!(op, Operator::BrIf { .. })));
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(1).unwrap(), 8);
        let stored = if matches!(boundary, Boundary::ArmStore) {
            23
        } else {
            0
        };
        assert_eq!(
            &instance.memory("state")[..8],
            &[0, 0, 0, 0, stored, 0, 0, 0]
        );
        let mut instance = module.instantiate();
        let expected = match boundary {
            Boundary::DifferentTuple => 109,
            Boundary::ArmStore => 107,
            _ => 7,
        };
        assert_eq!(instance.call::<i32>(0).unwrap(), expected);
    }
}

#[test]
fn shared_tail_conditions_preserve_load_and_call_snapshots() {
    for boundary in [Boundary::LoadCondition, Boundary::CallCondition] {
        let module = ordinary_tail(boundary);
        let operators = entry_operators(&module);
        assert!(operators
            .iter()
            .any(|op| matches!(op, Operator::BrIf { .. })));
        assert!(!operators.iter().any(|op| matches!(op, Operator::If { .. })));
        for input in [0, 1] {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>(input).unwrap(), input + 7);
            assert_eq!(&instance.memory("state")[..8], &[0; 8]);
        }
    }
}

#[test]
fn a_shared_tail_evaluates_its_condition_before_capturing_the_outgoing_tuple() {
    let mut fixture = Fixture::new();
    let trapped_value = fixture
        .program
        .function(signature(&[], &[Type::I32]), |body| body.trap())
        .unwrap();
    let module = fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let divisor = body.parameter::<I32>(0)?;
        let result = body.block::<I32>(|mut outer, exit| {
            let result = outer.block::<I32>(|mut inner, _| {
                // This pure call is demanded by both outgoing edges. Its authored
                // trap must follow the division used to choose between them.
                let outgoing = inner.call::<I32>(trapped_value, &[])?;
                let condition = divisor.unsigned().div(&divisor).ne(0);
                inner.branch_if(condition, &exit, &outgoing)?;
                inner.yield_(outgoing)
            })?;
            outer.yield_(result.add(100))
        })?;
        body.return_(result)
    });
    assert!(entry_operators(&module)
        .iter()
        .any(|op| matches!(op, Operator::BrIf { .. })));
    assert_eq!(
        module.instantiate().call::<i32>(0),
        Err(wasmtime::Trap::IntegerDivisionByZero)
    );
    assert_eq!(
        module.instantiate().call::<i32>(1),
        Err(wasmtime::Trap::UnreachableCodeReached)
    );
}

#[test]
fn a_dead_exit_channel_does_not_change_the_backedge_stack_shape() {
    let module = Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let result = body.loop_::<(I32, I32), (I32, I32)>(
            (count, 0),
            |mut iteration, labels, (left, sum)| {
                let next = (left.sub(1), sum.add(3));
                iteration.branch_if(next.0.ne(0), &labels.again, next.clone())?;
                iteration.yield_(next)
            },
        )?;
        body.return_(result.1)
    });
    let operators = entry_operators(&module);
    assert!(operators.iter().any(|op| matches!(op, Operator::If { .. })));
    assert!(!operators
        .iter()
        .any(|op| matches!(op, Operator::BrIf { .. })));
    assert_eq!(module.instantiate().call::<i32>(3).unwrap(), 9);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn conditional_backedges_execute_in_v8() {
    assert_eq!(
        swapped_tail().run_v8(&Input::call("run", &[Value::I32(5)])),
        Observation::returned(&[
            Value::I32(0),
            Value::I32(7),
            Value::I32(3),
            Value::I32(3),
            Value::I64(123)
        ])
    );
}
