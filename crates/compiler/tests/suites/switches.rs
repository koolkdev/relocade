use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, MemoryBytes, TestModule, Value};

use wasm86_compiler::{BuildError, FunctionImport, MemoryImport, Program, Type, I1, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

fn dense_values() -> TestModule {
    let fixture = Fixture::new();
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let selector = body.parameter::<I32>(0)?;
        let result = body.switch_value::<I32, _>(&selector, &[10, 11, 12, 13], |arm, key| {
            arm.yield_(match key {
                Some(10) => 41,
                Some(11) => 43,
                Some(12) => 47,
                Some(13) => 53,
                _ => 97,
            })
        })?;
        body.return_(result)
    })
}

fn sparse_endpoints() -> TestModule {
    let fixture = Fixture::new();
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let selector = body.parameter::<I32>(0)?;
        let result =
            body.switch_value::<I32, _>(&selector, &[0xffff_ffff, 0, 0x8000_0000], |arm, key| {
                arm.yield_(match key {
                    Some(0) => 11,
                    Some(0x8000_0000) => 13,
                    Some(0xffff_ffff) => 17,
                    _ => 19,
                })
            })?;
        body.return_(result)
    })
}

fn narrow_selector() -> TestModule {
    let fixture = Fixture::new();
    fixture.function(&[Type::I8], &[Type::I32], |mut body| {
        let selector = body.parameter::<I8>(0)?.add(1);
        let result = body.switch_value::<I32, _>(&selector, &[0, 1, 255], |arm, key| {
            arm.yield_(match key {
                Some(0) => 42,
                Some(1) => 43,
                Some(255) => 47,
                _ => 53,
            })
        })?;
        body.return_(result)
    })
}

fn narrow_result() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5, 0xa5, 0xa5, 0xa5]);
    fixture.function(&[Type::I32, Type::I8], &[Type::I8], |mut body| {
        let selector = body.parameter::<I32>(0)?;
        let input = body.parameter::<I8>(1)?;
        let result = body.switch_value::<I8, _>(selector, &[0, 1], |arm, key| {
            arm.yield_(input.add(if key == Some(0) { 1 } else { 2 }))
        })?;
        body.store(state, 0, &result)?;
        body.return_(result)
    })
}

fn nested_value_and_exit() -> TestModule {
    let fixture = Fixture::new();
    fixture.function(
        &[Type::I32, Type::I1, Type::I64],
        &[Type::I64],
        |mut body| {
            let selector = body.parameter::<I32>(0)?;
            let condition = body.parameter::<I1>(1)?;
            let input = body.parameter::<I64>(2)?;
            let result = body.switch_value::<I64, _>(selector, &[0, 1], |mut arm, key| {
                if key == Some(0) {
                    let nested = arm.if_value::<I64>(
                        &condition,
                        |branch| branch.yield_(&input),
                        |branch| branch.yield_(0x8000_0000_0000_0000u64),
                    )?;
                    arm.yield_(nested)
                } else {
                    arm.return_(0xffff_ffff_ffff_ffffu64)
                }
            })?;
            body.return_(result)
        },
    )
}

fn selected_effects_and_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I32]),
        &[Value::I32(41)],
    );
    let mutate = fixture.program.declare(signature(&[], &[Type::I32]));
    let mut mutation = fixture.program.define(mutate).unwrap();
    mutation.store::<I32>(state, 0, 13).unwrap();
    mutation.return_(99).unwrap();
    let run = fixture
        .program
        .declare(signature(&[Type::I32, Type::I32], &[Type::I32]));
    let mut body = fixture.program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let address = body.parameter::<I32>(1).unwrap();
    let before = body.load::<I32>(state, 0).unwrap();
    body.switch(&selector, &[0, 1, 2], |mut arm, key| match key {
        Some(0) => arm.store::<I32>(state, 0, 9),
        Some(1) => {
            let value = arm.load_at::<I32>(state, &address, 0)?;
            arm.store(state, 4, value)
        }
        Some(2) => {
            let _unused = arm.call::<I32>(mutate, &[])?;
            Ok(())
        }
        _ => arm.tail_call(receive, &[before.argument()]),
    })
    .unwrap();
    let after = body.load::<I32>(state, 0).unwrap();
    body.store::<I32>(state, 8, 11).unwrap();
    body.return_(before.add(after)).unwrap();
    fixture.finish(run)
}

fn shared_call_result() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I32]),
        &[Value::I32(23)],
    );
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let selector = body.parameter::<I32>(0)?;
        let result = body.switch_value::<I32, _>(selector, &[0, 1], |mut arm, key| {
            arm.store::<I32>(state, 0, key.map_or(3, |key| key + 1))?;
            let answer = arm.call::<I32>(receive, &[9.into()])?;
            arm.yield_(answer.add(1))
        })?;
        body.store(state, 4, &result)?;
        body.return_(result.add(&result))
    })
}

fn unused_result_keeps_effects() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I32]),
        &[Value::I32(23)],
    );
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let selector = body.parameter::<I32>(0)?;
        let _unused = body.switch_value::<I32, _>(selector, &[1, 2], |mut arm, key| {
            arm.store::<I32>(state, 0, key.unwrap_or(3))?;
            if key == Some(1) {
                let _answer = arm.call::<I32>(receive, &[9.into()])?;
            }
            let unused_load = arm.load::<I32>(state, 65536)?;
            arm.yield_(unused_load)
        })?;
        body.return_(17)
    })
}

fn empty_default() -> TestModule {
    let fixture = Fixture::new();
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let selector = body.parameter::<I32>(0)?;
        let mut visits = 0;
        let value = body.switch_value::<I32, _>(selector, &[], |arm, key| {
            assert_eq!(key, None);
            visits += 1;
            arm.yield_(23)
        })?;
        assert_eq!(visits, 1);
        body.return_(value)
    })
}

fn failed_switch_discards_effects() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0]);
    let run = fixture
        .program
        .declare(signature(&[Type::I32], &[Type::I32]));
    let mut body = fixture.program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let mut visited_default = false;
    let failed = body.switch(selector, &[0, 1], |mut arm, key| match key {
        Some(0) => {
            let receive = arm.program().import_function(FunctionImport {
                module: "test".into(),
                name: "receive".into(),
                signature: signature(&[Type::I32], &[Type::I32]),
            });
            let _answer = arm.call::<I32>(receive, &[9.into()])?;
            arm.store::<I32>(state, 0, 9)
        }
        Some(1) => arm.return_(9u64),
        _ => {
            visited_default = true;
            Ok(())
        }
    });
    assert_eq!(
        failed.err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I32,
            actual: Type::I64,
        })
    );
    assert!(!visited_default);
    body.store::<I32>(state, 4, 17).unwrap();
    body.return_(31).unwrap();
    fixture.finish(run)
}

#[derive(Debug, PartialEq)]
enum Event {
    Load(u64),
    Store(u64),
    Table(usize),
    Call,
    Tail,
}

fn inspect_run(bytes: &[u8]) -> Vec<Event> {
    Validator::new().validate_all(bytes).unwrap();
    let mut imports = 0;
    let mut run = None;
    let mut functions = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(section) => {
                for import in section {
                    if matches!(import.unwrap().ty, TypeRef::Func(_)) {
                        imports += 1;
                    }
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.name == "run" {
                        run = Some(export.index as usize);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let mut events = Vec::new();
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::I32Load { memarg } => events.push(Event::Load(memarg.offset)),
                        Operator::I32Store { memarg } | Operator::I32Store8 { memarg } => {
                            events.push(Event::Store(memarg.offset))
                        }
                        Operator::BrTable { targets } => {
                            events.push(Event::Table(targets.len() as usize))
                        }
                        Operator::Call { .. } => events.push(Event::Call),
                        Operator::ReturnCall { .. } => events.push(Event::Tail),
                        _ => {}
                    }
                }
                functions.push(events);
            }
            _ => {}
        }
    }
    functions.remove(run.unwrap() - imports)
}

#[test]
fn dense_dispatch_uses_a_table_and_sparse_endpoints_remain_bounded() {
    assert!(inspect_run(dense_values().bytes()).contains(&Event::Table(4)));
    let sparse = sparse_endpoints();
    inspect_run(sparse.bytes());
    assert!(
        sparse.bytes().len() < 2048,
        "sparse keys must not allocate their numeric span"
    );
}

#[test]
fn switches_capture_prior_reads_across_selected_stores_and_calls() {
    let events = inspect_run(selected_effects_and_snapshot().bytes());
    let dispatch = events
        .iter()
        .position(|event| matches!(event, Event::Table(_)))
        .unwrap();
    assert_eq!(events[0], Event::Load(0));
    assert!(events[dispatch + 1..].contains(&Event::Store(0)));
    assert!(events[dispatch + 1..].contains(&Event::Call));
    assert!(events[dispatch + 1..].contains(&Event::Tail));
}

#[test]
fn unused_switch_results_drop_loads_but_retain_selected_effects() {
    let events = inspect_run(unused_result_keeps_effects().bytes());
    assert!(!events.iter().any(|event| matches!(event, Event::Load(_))));
    assert!(events.contains(&Event::Call));
    assert!(events.contains(&Event::Store(0)));
}

#[test]
fn typed_and_nested_switch_results_form_valid_modules() {
    for bytes in [
        narrow_selector(),
        narrow_result(),
        nested_value_and_exit(),
        shared_call_result(),
    ] {
        inspect_run(bytes.bytes());
    }
}

#[test]
fn values_from_completed_switch_arms_cannot_escape_their_scope() {
    let mut program = Program::new();
    let state = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let run = program.declare(signature(&[Type::I32], &[Type::I32]));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let mut child = None;
    body.switch(selector, &[0], |mut arm, key| {
        if key == Some(0) {
            child = Some(arm.load::<I32>(state, 0)?);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        body.value::<I32>(child.unwrap()).err(),
        Some(BuildError::OutOfScope)
    );
    body.return_(17).unwrap();
    program.export("run", run).unwrap();
    inspect_run(&program.compile().unwrap());
}

#[test]
fn empty_cases_build_only_the_default_and_need_no_table() {
    let events = inspect_run(empty_default().bytes());
    assert!(!events.iter().any(|event| matches!(event, Event::Table(_))));
}

#[test]
fn selectors_and_keys_are_validated_before_arm_construction() {
    let mut program = Program::new();
    let run = program.declare(signature(&[Type::I8], &[Type::I32]));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I8>(0).unwrap();
    let mut calls = 0;
    assert_eq!(
        body.switch(&selector, &[1, 1], |_, _| {
            calls += 1;
            Ok(())
        })
        .err(),
        Some(BuildError::DuplicateSwitchCase { key: 1 })
    );
    assert_eq!(
        body.switch(&selector, &[256], |_, _| {
            calls += 1;
            Ok(())
        })
        .err(),
        Some(BuildError::SwitchCaseOutOfRange {
            key: 256,
            selector: Type::I8
        })
    );
    let mut foreign_program = Program::new();
    let foreign_run = foreign_program.declare(signature(&[Type::I32], &[Type::I32]));
    let foreign_body = foreign_program.define(foreign_run).unwrap();
    let foreign = foreign_body.parameter::<I32>(0).unwrap();
    assert_eq!(
        body.switch(foreign, &[1, 1], |_, _| {
            calls += 1;
            Ok(())
        })
        .err(),
        Some(BuildError::ForeignBody)
    );
    assert_eq!(calls, 0);
    foreign_body.return_(0).unwrap();
    body.return_(17).unwrap();
    program.export("run", run).unwrap();
    inspect_run(&program.compile().unwrap());
}

#[test]
fn value_switches_require_a_yield_and_completed_arms() {
    let mut program = Program::new();
    let run = program.declare(signature(&[Type::I32], &[Type::I32]));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let incomplete = body.switch_value::<I32, _>(&selector, &[0], |arm, key| {
        if key == Some(0) {
            arm.yield_(7)
        } else {
            Ok(())
        }
    });
    assert_eq!(incomplete.err(), Some(BuildError::IncompleteBranch));
    let no_value = body.switch_value::<I8, _>(&selector, &[0], |arm, _| arm.return_(11));
    assert_eq!(no_value.err(), Some(BuildError::MissingBranchValue));
    assert_eq!(
        body.switch(selector, &[0], |arm, _| arm.yield_(7)).err(),
        Some(BuildError::InvalidYield)
    );
    body.return_(17).unwrap();
    program.export("run", run).unwrap();
    inspect_run(&program.compile().unwrap());
}

#[test]
fn a_later_arm_failure_removes_earlier_effects_and_unused_imports() {
    let bytes = failed_switch_discards_effects();
    assert_eq!(inspect_run(bytes.bytes()), [Event::Store(4)]);
    for payload in Parser::new(0).parse_all(bytes.bytes()) {
        if let Payload::ImportSection(section) = payload.unwrap() {
            for import in section {
                assert!(!matches!(import.unwrap().ty, TypeRef::Func(_)));
            }
        }
    }
}

#[test]
fn empty_switches_execute_the_default() {
    let empty = empty_default();
    let mut instance = empty.instantiate();
    assert_eq!(instance.call::<i32>(-1), Ok(23));
}

#[test]
fn failed_switch_builds_discard_authored_effects_at_runtime() {
    let failed = failed_switch_discards_effects();
    let mut instance = failed.instantiate();
    assert_eq!(instance.call::<i32>(0), Ok(31));
    assert_eq!(&instance.memory("state")[..8], &[7, 0, 0, 0, 0x11, 0, 0, 0]);
}

#[test]
fn dense_switches_handle_cases_and_default_boundaries() {
    let dense = dense_values();
    for (input, result) in [
        (10, 41),
        (11, 43),
        (12, 47),
        (13, 53),
        (9, 97),
        (14, 97),
        (-1, 97),
    ] {
        let mut instance = dense.instantiate();
        assert_eq!(instance.call::<i32>(input), Ok(result));
    }
}

#[test]
fn sparse_switches_handle_unsigned_endpoints() {
    let sparse = sparse_endpoints();
    for (input, result) in [
        (0, 11),
        (-2147483648, 13),
        (-1, 17),
        (2147483647, 19),
        (1, 19),
    ] {
        let mut instance = sparse.instantiate();
        assert_eq!(instance.call::<i32>(input), Ok(result));
    }
}

#[test]
fn narrow_switch_selectors_are_canonical() {
    let selector = narrow_selector();
    for (input, result) in [(255, 42), (254, 47), (0, 43), (1, 53)] {
        let mut instance = selector.instantiate();
        assert_eq!(instance.call::<i32>(input), Ok(result));
    }
}

#[test]
fn narrow_switch_results_are_canonical() {
    let narrow = narrow_result();
    let mut instance = narrow.instantiate();
    assert_eq!(instance.call::<i32>((0, 255)), Ok(0));
    assert_eq!(&instance.memory("state")[..4], &[0, 0xa5, 0xa5, 0xa5]);
    let mut instance = narrow.instantiate();
    assert_eq!(instance.call::<i32>((2, 255)), Ok(1));
    assert_eq!(&instance.memory("state")[..4], &[1, 0xa5, 0xa5, 0xa5]);
}

#[test]
fn nested_switch_values_and_exits_preserve_i64_results() {
    let nested = nested_value_and_exit();
    for (selector, condition, result) in [
        (0, 1, i64::MAX),
        (0, 0, i64::MIN),
        (1, 0, -1_i64),
        (2, 1, -1_i64),
    ] {
        let mut instance = nested.instantiate();
        assert_eq!(
            instance.call::<i64>((selector, condition, i64::MAX)),
            Ok(result)
        );
    }
}

#[test]
fn switches_execute_only_selected_effects_and_snapshots() {
    let effects = selected_effects_and_snapshot();
    for (selector, address, expected_result, expected_memory, expected_callbacks) in [
        (
            0,
            65536,
            Some(16),
            &[9, 0, 0, 0, 5, 0, 0, 0, 0x0b, 0, 0, 0],
            vec![],
        ),
        (
            1,
            0,
            Some(14),
            &[7, 0, 0, 0, 7, 0, 0, 0, 0x0b, 0, 0, 0],
            vec![],
        ),
        (
            1,
            65536,
            None,
            &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0],
            vec![],
        ),
        (
            2,
            65536,
            Some(20),
            &[0x0d, 0, 0, 0, 5, 0, 0, 0, 0x0b, 0, 0, 0],
            vec![],
        ),
        (
            3,
            65536,
            Some(41),
            &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0],
            vec![
                Call::new("receive", &[Value::I32(7)]).with_memories(&[MemoryBytes::new(
                    "state",
                    &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0],
                )]),
            ],
        ),
    ] {
        let mut instance = effects.instantiate();
        assert_eq!(
            instance.call::<i32>((selector, address)).ok(),
            expected_result
        );
        assert_eq!(&instance.memory("state")[..12], expected_memory);
        assert_eq!(instance.callbacks(), expected_callbacks);
    }
}

#[test]
fn shared_switch_call_results_execute_once() {
    let shared = shared_call_result();
    let mut instance = shared.instantiate();
    assert_eq!(instance.call::<i32>(1), Ok(48));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(9)])
            .with_memories(&[MemoryBytes::new("state", &[2, 0, 0, 0, 5, 0, 0, 0])])]
    );
    assert_eq!(&instance.memory("state")[..8], &[2, 0, 0, 0, 0x18, 0, 0, 0]);
}

#[test]
fn unused_switch_results_preserve_selected_effects() {
    let unused = unused_result_keeps_effects();
    let mut instance = unused.instantiate();
    assert_eq!(instance.call::<i32>(1), Ok(17));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(9)])
            .with_memories(&[MemoryBytes::new("state", &[1, 0, 0, 0])])]
    );
    assert_eq!(&instance.memory("state")[..4], &[1, 0, 0, 0]);
    let mut instance = unused.instantiate();
    assert_eq!(instance.call::<i32>(9), Ok(17));
    assert!(instance.callbacks().is_empty());
    assert_eq!(&instance.memory("state")[..4], &[3, 0, 0, 0]);
}
