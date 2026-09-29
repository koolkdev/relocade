use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, Callback, Input, MemoryBytes, Observation, TestModule, Value};

use wasm86_compiler::{BuildError, Program, Type, Val, I1, I32, I64, I8};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, Validator};

fn ordered_imports() -> TestModule {
    let mut fixture = Fixture::new();
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32, Type::I64], &[Type::I64]),
        &[Value::I64(i64::MAX)],
    );
    fixture.function(&[Type::I32, Type::I64], &[Type::I64], |mut body| {
        let word = body.parameter::<I32>(0)?.add(1);
        let wide = body.parameter::<I64>(1)?.add(1);
        let arguments = [word.argument(), wide.argument()];
        let first = body.call::<I64>(receive, &arguments)?;
        let _second = body.call::<I64>(receive, &arguments)?;
        body.return_(first.add(&first))
    })
}

fn narrow_arguments_and_result() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5, 0x5a]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I8, Type::I8], &[Type::I8]),
        &[Value::I32(255)],
    );
    fixture.function(&[Type::I8], &[Type::I8], |mut body| {
        let raw = body.parameter::<I8>(0)?.add(1);
        body.store(state, 0, &raw)?;
        let answer = body.call::<I8>(receive, &[raw.argument(), raw.argument()])?;
        body.return_(answer.add(1))
    })
}

fn transitive_mutation() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]);
    let run = fixture.program.declare(signature(&[], &[Type::I32]));
    let wrapper = fixture.program.declare(signature(&[], &[Type::I32]));
    let mutator = fixture.program.declare(signature(&[], &[Type::I32]));
    fixture
        .program
        .define(run, |mut body| {
            let before = body.load::<I32>(state, 0).unwrap();
            let answer = body.call::<I32>(wrapper, &[]).unwrap();
            body.store(state, 4, &answer).unwrap();
            let after = body.load::<I32>(state, 0).unwrap();
            body.return_(before.add(&after))
        })
        .unwrap();
    fixture
        .program
        .define(wrapper, |body| body.tail_call(mutator, &[]))
        .unwrap();
    fixture
        .program
        .define(mutator, |mut body| {
            body.store::<I32>(state, 0, 9).unwrap();
            body.return_(5)
        })
        .unwrap();
    fixture.finish(run)
}

enum ReadUse {
    Discard,
    ReturnSnapshot,
    AddFreshRead,
}

fn readonly_call(
    offset: u32,
    store_offset: u32,
    result_use: ReadUse,
    initial: &[u8],
) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", initial);
    let run = fixture.program.declare(signature(&[], &[Type::I32]));
    let reader = fixture.program.declare(signature(&[], &[Type::I32]));
    fixture
        .program
        .define(run, |mut body| {
            let before = body.call::<I32>(reader, &[]).unwrap();
            if store_offset != 0 {
                body.store::<I32>(state, 0, 1).unwrap();
            }
            body.store::<I32>(state, store_offset, 9).unwrap();
            match result_use {
                ReadUse::Discard => body.return_(7),
                ReadUse::ReturnSnapshot => body.return_(&before),
                ReadUse::AddFreshRead => {
                    let after = body.call::<I32>(reader, &[]).unwrap();
                    body.return_(before.add(&after))
                }
            }
        })
        .unwrap();
    fixture
        .program
        .define(reader, |mut body| {
            let value = body.load::<I32>(state, offset).unwrap();
            body.return_(&value)
        })
        .unwrap();
    fixture.finish(run)
}

fn computed_helper_read(initial: &[u8]) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", initial);
    let run = fixture
        .program
        .declare(signature(&[Type::I32], &[Type::I32]));
    let reader = fixture
        .program
        .declare(signature(&[Type::I32], &[Type::I32]));
    fixture
        .program
        .define(run, |mut body| {
            let address = body.parameter::<I32>(0).unwrap();
            let before = body.call::<I32>(reader, &[address.argument()]).unwrap();
            body.store::<I32>(state, 0, 9).unwrap();
            body.return_(&before)
        })
        .unwrap();
    fixture
        .program
        .define(reader, |mut body| {
            let address = body.parameter::<I32>(0).unwrap();
            let loaded = body.load_at::<I32>(state, &address, 0).unwrap();
            body.return_(&loaded)
        })
        .unwrap();
    fixture.finish(run)
}

fn predicate_result() -> TestModule {
    let mut fixture = Fixture::new();
    let run = fixture
        .program
        .declare(signature(&[Type::I32], &[Type::I32]));
    let predicate = fixture
        .program
        .declare(signature(&[Type::I32], &[Type::I1]));
    fixture
        .program
        .define(run, |mut body| {
            let input = body.parameter::<I32>(0).unwrap();
            let condition = body.call::<I1>(predicate, &[input.argument()]).unwrap();
            body.if_(&condition, |branch| branch.return_(11)).unwrap();
            body.return_(22)
        })
        .unwrap();
    fixture
        .program
        .define(predicate, |body| {
            let input = body.parameter::<I32>(0).unwrap();
            body.return_(input.eq(7))
        })
        .unwrap();
    fixture.finish(run)
}

fn branch_call() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]);
    let run = fixture
        .program
        .declare(signature(&[Type::I1], &[Type::I32]));
    let helper = fixture.program.declare(signature(&[], &[Type::I32]));
    fixture
        .program
        .define(run, |mut body| {
            let condition = body.parameter::<I1>(0).unwrap();
            body.store::<I32>(state, 0, 1).unwrap();
            body.if_(&condition, |mut branch| {
                let _unused = branch.call::<I32>(helper, &[])?;
                Ok(())
            })
            .unwrap();
            body.store::<I32>(state, 4, 2).unwrap();
            body.return_(17)
        })
        .unwrap();
    fixture
        .program
        .define(helper, |mut body| {
            body.store::<I32>(state, 8, 3).unwrap();
            body.trap()
        })
        .unwrap();
    fixture.finish(run)
}

fn snapshot_with_a_tail_call_branch(store_offset: u32) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]);
    let run = fixture
        .program
        .declare(signature(&[Type::I1], &[Type::I32]));
    let wrapper = fixture.program.declare(signature(&[], &[Type::I32]));
    let mutator = fixture.program.declare(signature(&[], &[Type::I32]));
    fixture
        .program
        .define(run, |mut body| {
            let condition = body.parameter::<I1>(0).unwrap();
            let before = body.load::<I32>(state, 4).unwrap();
            body.store::<I32>(state, store_offset, 1).unwrap();
            body.if_(&condition, |branch| branch.tail_call(wrapper, &[]))
                .unwrap();
            body.return_(&before)
        })
        .unwrap();
    fixture
        .program
        .define(wrapper, |body| body.tail_call(mutator, &[]))
        .unwrap();
    fixture
        .program
        .define(mutator, |mut body| {
            body.store::<I32>(state, 4, 9).unwrap();
            body.return_(5)
        })
        .unwrap();
    fixture.finish(run)
}

fn argument_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I32]),
        &[Value::I32(17)],
    );
    fixture.function(&[], &[Type::I32], |mut body| {
        let argument = body.load::<I32>(state, 0)?;
        body.store::<I32>(state, 0, 1)?;
        let answer = body.call::<I32>(receive, &[argument.argument()])?;
        body.store::<I32>(state, 4, 2)?;
        body.return_(&answer)
    })
}

#[derive(Debug, PartialEq)]
enum Event {
    Call,
    Tail,
    Return,
    If,
    End,
    Load(u64),
    Store(u64),
}

#[derive(Default)]
struct Code {
    events: Vec<Event>,
    additions: usize,
    masks: usize,
    drops: usize,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut imports = 0;
    let mut exported = None;
    let mut index = 0;
    let mut code = Code::default();
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
                        assert_eq!(export.kind, ExternalKind::Func);
                        exported = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let current = index + imports;
                index += 1;
                if Some(current) != exported {
                    continue;
                }
                let mut depth = 0;
                let mut operators = body.get_operators_reader().unwrap();
                while !operators.eof() {
                    let event = match operators.read().unwrap() {
                        Operator::Call { .. } => Event::Call,
                        Operator::Return => Event::Return,
                        Operator::If { .. } => {
                            depth += 1;
                            Event::If
                        }
                        Operator::End if depth > 0 => {
                            depth -= 1;
                            Event::End
                        }
                        Operator::I32Load { memarg } => Event::Load(memarg.offset),
                        Operator::I32Store { memarg } | Operator::I32Store8 { memarg } => {
                            Event::Store(memarg.offset)
                        }
                        Operator::I32Add | Operator::I64Add => {
                            code.additions += 1;
                            continue;
                        }
                        Operator::Drop => {
                            code.drops += 1;
                            continue;
                        }
                        Operator::I32And => {
                            code.masks += 1;
                            continue;
                        }
                        Operator::ReturnCall { .. } => Event::Tail,
                        _ => continue,
                    };
                    code.events.push(event);
                }
            }
            _ => {}
        }
    }
    assert!(exported.is_some_and(|export| export >= imports && export < index + imports));
    code
}

#[test]
fn imported_calls_execute_once_each_even_without_memory_or_a_used_result() {
    let code = inspect(ordered_imports().bytes());
    assert_eq!(code.events, [Event::Call, Event::Call, Event::Return]);
    assert_eq!(code.additions, 3);
    assert_eq!(code.drops, 1);
}

#[test]
fn calls_depending_on_recursion_are_retained_when_unused() {
    let mut program = Program::new();
    let recursive = program.declare(signature(&[], &[Type::I32]));
    let wrapper = program.declare(signature(&[], &[Type::I32]));
    let run = program.declare(signature(&[], &[Type::I32]));
    for (function, target) in [(recursive, recursive), (wrapper, recursive), (run, wrapper)] {
        program
            .define(function, |mut body| {
                let _unused = body.call::<I32>(target, &[]).unwrap();
                body.return_(7)
            })
            .unwrap();
    }
    program.export("run", run).unwrap();
    let code = inspect(&program.compile().unwrap());
    assert_eq!(code.events, [Event::Call, Event::Return]);
    assert_eq!(code.drops, 1);
}

#[test]
fn repeated_narrow_arguments_share_normalization_and_results_are_canonical() {
    let code = inspect(narrow_arguments_and_result().bytes());
    assert_eq!(code.events, [Event::Store(0), Event::Call, Event::Return]);
    assert_eq!((code.additions, code.masks), (2, 2));
}

#[test]
fn transitive_callee_writes_preserve_earlier_loads() {
    let code = inspect(transitive_mutation().bytes());
    assert_eq!(
        code.events,
        [
            Event::Load(0),
            Event::Call,
            Event::Store(4),
            Event::Load(0),
            Event::Return
        ]
    );
}

#[test]
fn readonly_calls_follow_result_demand_without_crossing_aliasing_writes() {
    assert_eq!(
        inspect(computed_helper_read(&[]).bytes()).events,
        [Event::Call, Event::Store(0), Event::Return]
    );
    assert_eq!(
        inspect(readonly_call(0, 0, ReadUse::ReturnSnapshot, &[]).bytes()).events,
        [Event::Call, Event::Store(0), Event::Return]
    );
    assert_eq!(
        inspect(readonly_call(8, 4, ReadUse::ReturnSnapshot, &[]).bytes()).events,
        [Event::Store(0), Event::Store(4), Event::Call, Event::Return]
    );
    assert_eq!(
        inspect(readonly_call(0, 0, ReadUse::Discard, &[]).bytes()).events,
        [Event::Store(0), Event::Return]
    );
    assert_eq!(
        inspect(readonly_call(0, 0, ReadUse::AddFreshRead, &[]).bytes()).events,
        [Event::Call, Event::Store(0), Event::Call, Event::Return]
    );
}

#[test]
fn an_effectful_helper_stays_on_its_selected_path() {
    assert_eq!(
        inspect(branch_call().bytes()).events,
        [
            Event::Store(0),
            Event::If,
            Event::Call,
            Event::End,
            Event::Store(4),
            Event::Return
        ]
    );
}

#[test]
fn a_false_edge_snapshot_precedes_an_aliasing_prefix_write() {
    assert_eq!(
        inspect(snapshot_with_a_tail_call_branch(4).bytes()).events,
        [
            Event::Load(4),
            Event::Store(4),
            Event::If,
            Event::Tail,
            Event::End,
            Event::Return,
        ]
    );
}

#[test]
fn a_logical_one_bit_call_result_can_control_a_branch() {
    let code = inspect(predicate_result().bytes());
    assert_eq!(
        code.events,
        [
            Event::Call,
            Event::If,
            Event::Return,
            Event::End,
            Event::Return
        ]
    );
    assert_eq!(code.masks, 0);
}

#[test]
fn standalone_typed_arguments_keep_their_logical_type() {
    let mut fixture = Fixture::new();
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I1], &[Type::I1]),
        &[Value::I32(1)],
    );
    let module = fixture.function(&[], &[Type::I1], |mut body| {
        assert_eq!(
            body.call::<I1>(receive, &[Val::<I8>::from(1).into()]).err(),
            Some(BuildError::TypeMismatch {
                expected: Type::I1,
                actual: Type::I8,
            })
        );
        let result = body.call::<I1>(receive, &[1.into()])?;
        body.return_(result)
    });
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 1);
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(1)])]
    );
}

#[test]
fn imported_calls_preserve_order_and_share_arguments_at_runtime() {
    let module = ordered_imports();
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i64>((i32::MAX, i64::MAX)).unwrap(), -2);
    assert_eq!(
        instance.callbacks(),
        [
            Call::new("receive", &[Value::I32(i32::MIN), Value::I64(i64::MIN)]),
            Call::new("receive", &[Value::I32(i32::MIN), Value::I64(i64::MIN)]),
        ],
    );
}

#[test]
fn narrow_calls_normalize_arguments_and_results_at_runtime() {
    let mut instance = narrow_arguments_and_result().instantiate();
    assert_eq!(instance.call::<i32>(255), Ok(0));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(0), Value::I32(0)])
            .with_memories(&[MemoryBytes::new("state", &[0, 0x5a])])]
    );
    assert_eq!(&instance.memory("state")[..2], &[0, 0x5a]);
}

#[test]
fn transitive_callee_writes_preserve_prior_snapshots_at_runtime() {
    let mut instance = transitive_mutation().instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(16));
    assert_eq!(
        &instance.memory("state")[..10],
        &[9, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn readonly_calls_follow_snapshot_aliasing_and_result_demand_at_runtime() {
    let mut instance = readonly_call(
        0,
        0,
        ReadUse::ReturnSnapshot,
        &[7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a],
    )
    .instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(7));
    assert_eq!(
        &instance.memory("state")[..10],
        &[9, 0, 0, 0, 0x0b, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = readonly_call(
        8,
        4,
        ReadUse::ReturnSnapshot,
        &[7, 0, 0, 0, 11, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a],
    )
    .instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(5));
    assert_eq!(
        &instance.memory("state")[..14],
        &[1, 0, 0, 0, 9, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = readonly_call(
        0,
        0,
        ReadUse::AddFreshRead,
        &[7, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a],
    )
    .instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(16));
    assert_eq!(
        &instance.memory("state")[..10],
        &[9, 0, 0, 0, 0x0b, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance =
        readonly_call(0, 0, ReadUse::Discard, &[7, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(7));
    assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn computed_helper_reads_preserve_aliased_pointer_snapshots_at_runtime() {
    let mut instance =
        computed_helper_read(&[7, 0, 0, 0, 11, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert_eq!(instance.call::<i32>(8), Ok(5));
    assert_eq!(
        &instance.memory("state")[..14],
        &[9, 0, 0, 0, 0x0b, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
    // A computed helper read conservatively aliases every byte in its memory.
    let mut instance = computed_helper_read(&[7, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert_eq!(instance.call::<i32>(0), Ok(7));
    assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn one_bit_call_results_control_branches_at_runtime() {
    let predicate = predicate_result();
    let mut instance = predicate.instantiate();
    assert_eq!(instance.call::<i32>(7), Ok(11));
    let mut instance = predicate.instantiate();
    assert_eq!(instance.call::<i32>(5), Ok(22));
}

#[test]
fn explicit_traps_in_helpers_terminate_only_the_selected_path() {
    let branch = branch_call();
    let mut instance = branch.instantiate();
    assert_eq!(instance.call::<i32>(0), Ok(17));
    assert_eq!(
        &instance.memory("state")[..14],
        &[1, 0, 0, 0, 2, 0, 0, 0, 0x0b, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = branch.instantiate();
    assert_eq!(
        instance.call::<i32>(1),
        Err(wasmtime::Trap::UnreachableCodeReached)
    );
    assert_eq!(
        &instance.memory("state")[..14],
        &[1, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn tail_call_branches_preserve_prefix_writes_and_false_edge_snapshots() {
    for (store_offset, condition, returned, memory) in [
        (0, 0, 11, [1, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]),
        (0, 1, 5, [1, 0, 0, 0, 9, 0, 0, 0, 0xa5, 0x5a]),
        (4, 0, 11, [7, 0, 0, 0, 1, 0, 0, 0, 0xa5, 0x5a]),
        (4, 1, 5, [7, 0, 0, 0, 9, 0, 0, 0, 0xa5, 0x5a]),
    ] {
        let mut instance = snapshot_with_a_tail_call_branch(store_offset).instantiate();
        assert_eq!(instance.call::<i32>(condition), Ok(returned));
        assert_eq!(&instance.memory("state")[..memory.len()], &memory);
    }
}

#[test]
fn call_arguments_preserve_their_snapshot_across_an_overlapping_store() {
    let mut instance = argument_snapshot().instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(17));
    assert_eq!(
        instance.callbacks(),
        &[
            Call::new("receive", &[Value::I32(7)]).with_memories(&[MemoryBytes::new(
                "state",
                &[1, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
            )])
        ]
    );
    assert_eq!(
        &instance.memory("state")[..10],
        &[1, 0, 0, 0, 2, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 smoke tests"]
fn v8_imported_calls_preserve_order_and_i64_values() {
    let module = ordered_imports();
    let input = Input::call("run", &[Value::I32(i32::MAX), Value::I64(i64::MAX)])
        .with_callbacks(&[Callback::new("receive", &[Value::I64(i64::MAX)])]);
    assert_eq!(
        module.run_v8(&input),
        Observation::returned(&[Value::I64(-2)]).with_callbacks(&[
            Call::new("receive", &[Value::I32(i32::MIN), Value::I64(i64::MIN)]),
            Call::new("receive", &[Value::I32(i32::MIN), Value::I64(i64::MIN)]),
        ]),
    );
}
