use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, MemoryBytes, TestModule, Value};

use wasm86_compiler::{IntType, Type, I1, I32, I64, I8};
use wasmparser::{BlockType, Operator, Parser, Payload, ValType, Validator};

fn direct_result<T: IntType>() -> TestModule {
    let fixture = Fixture::new();
    fixture.function(&[Type::I1, T::TYPE, T::TYPE], &[T::TYPE], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let yes = body.parameter::<T>(1)?;
        let no = body.parameter::<T>(2)?;
        let result =
            body.if_value::<T>(&condition, |arm| arm.yield_(&yes), |arm| arm.yield_(&no))?;
        body.return_(&result)
    })
}

fn shared_result() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?;
        let result = body.if_value::<I32>(
            &condition,
            |mut arm| {
                arm.store::<I32>(state, 0, 1)?;
                arm.yield_(input.add(1))
            },
            |mut arm| {
                arm.store::<I32>(state, 0, 2)?;
                arm.yield_(input.add(2))
            },
        )?;
        body.store(state, 4, &result)?;
        body.return_(result.add(&result))
    })
}

fn selected_memory() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let address = body.parameter::<I32>(1)?;
        body.store::<I32>(state, 0, 1)?;
        let result = body.if_value::<I32>(
            &condition,
            |mut arm| {
                arm.store::<I32>(state, 4, 2)?;
                let value = arm.load_at::<I32>(state, &address, 0)?;
                arm.yield_(&value)
            },
            |mut arm| {
                arm.store::<I32>(state, 4, 3)?;
                arm.yield_(11)
            },
        )?;
        body.store::<I32>(state, 8, 4)?;
        body.return_(&result)
    })
}

fn unused_result() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I32]),
        &[Value::I32(23)],
    );
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let _unused = body.if_value::<I32>(
            &condition,
            |mut arm| {
                arm.store::<I32>(state, 0, 1)?;
                let _answer = arm.call::<I32>(receive, &[9.into()])?;
                let loaded = arm.load::<I32>(state, 65536)?;
                arm.yield_(&loaded)
            },
            |mut arm| {
                arm.store::<I32>(state, 0, 2)?;
                let loaded = arm.load::<I32>(state, 65536)?;
                arm.yield_(&loaded)
            },
        )?;
        body.store::<I32>(state, 4, 3)?;
        body.return_(17)
    })
}

fn snapshots() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let before = body.load::<I32>(state, 0)?;
        let result = body.if_value::<I32>(
            &condition,
            |mut arm| {
                arm.store::<I32>(state, 0, 9)?;
                let loaded = arm.load::<I32>(state, 4)?;
                arm.yield_(loaded.add(&before))
            },
            |mut arm| {
                arm.store::<I32>(state, 4, 3)?;
                let loaded = arm.load::<I32>(state, 0)?;
                arm.yield_(loaded.add(&before))
            },
        )?;
        body.store::<I32>(state, 0, 12)?;
        body.return_(result.add(&before))
    })
}

fn nested_fault() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I64],
        |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let fault = body.parameter::<I1>(1)?;
            let address = body.parameter::<I32>(2)?;
            let result = body.if_value::<I32>(
                &condition,
                |mut arm| {
                    arm.if_(&fault, |exit| exit.return_(0x8000_0000_0000_0000u64))?;
                    let loaded = arm.load_at::<I32>(state, &address, 0)?;
                    arm.yield_(&loaded)
                },
                |arm| arm.yield_(11),
            )?;
            body.return_(result.unsigned().extend::<I64>())
        },
    )
}

fn narrow_result() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5, 0x5a]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I8, Type::I8], &[Type::I64]),
        &[Value::I64(-9223372036854775808)],
    );
    fixture.function(&[Type::I1, Type::I8], &[Type::I64], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let input = body.parameter::<I8>(1)?;
        let result = body.if_value::<I8>(
            &condition,
            |arm| arm.yield_(input.add(1)),
            |arm| arm.yield_(7),
        )?;
        body.store(state, 0, &result)?;
        body.tail_call(receive, &[result.argument(), result.argument()])
    })
}

fn predicate_result() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1, Type::I1], &[Type::I1], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let input = body.parameter::<I1>(1)?;
        let result = body.if_value::<I1>(
            &condition,
            |arm| arm.yield_(input.add(1)),
            |arm| arm.yield_(false),
        )?;
        body.if_(&result, |mut arm| arm.store::<I32>(state, 0, 9))?;
        body.return_(&result)
    })
}

fn nested_result() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let outer = body.parameter::<I1>(0)?;
            let inner = body.parameter::<I1>(1)?;
            let shared = body.parameter::<I32>(2)?.add(1);
            let result = body.if_value::<I32>(
                &outer,
                |mut arm| {
                    let result = arm.if_value::<I32>(
                        &inner,
                        |child| child.yield_(shared.add(1)),
                        |child| child.yield_(shared.add(2)),
                    )?;
                    arm.yield_(&result)
                },
                |arm| arm.yield_(shared.add(3)),
            )?;
            body.store(state, 0, &shared)?;
            body.return_(result.add(&shared))
        },
    )
}

fn returning_arm() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1], &[Type::I64], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let result = body.if_value::<I32>(
            &condition,
            |arm| arm.return_(0x8000_0000_0000_0000u64),
            |arm| arm.yield_(7),
        )?;
        body.store::<I32>(state, 0, 9)?;
        body.return_(result.unsigned().extend::<I64>())
    })
}

fn tailing_arm() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I64]),
        &[Value::I64(-9223372036854775808)],
    );
    fixture.function(&[Type::I1], &[Type::I64], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let result = body.if_value::<I32>(
            &condition,
            |arm| arm.yield_(11),
            |mut arm| {
                arm.store::<I32>(state, 0, 1)?;
                arm.tail_call(receive, &[9.into()])
            },
        )?;
        body.store::<I32>(state, 4, 2)?;
        body.return_(result.unsigned().extend::<I64>())
    })
}

#[derive(Debug, PartialEq)]
enum Event {
    If(BlockType),
    Else,
    End,
    Load(u64),
    Store(u64),
    Mask,
    Call,
    Tail,
    Return,
}

#[derive(Default, Debug)]
struct Code {
    events: Vec<Event>,
    locals: u32,
    writes: usize,
    adds: usize,
    drops: usize,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            bodies += 1;
            for local in body.get_locals_reader().unwrap() {
                code.locals += local.unwrap().0;
            }
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                let event = match reader.read().unwrap() {
                    Operator::If { blockty } => Some(Event::If(blockty)),
                    Operator::Else => Some(Event::Else),
                    Operator::End if !reader.eof() => Some(Event::End),
                    Operator::I32Load { memarg } => Some(Event::Load(memarg.offset)),
                    Operator::I32Store { memarg } | Operator::I32Store8 { memarg } => {
                        Some(Event::Store(memarg.offset))
                    }
                    Operator::I32And => Some(Event::Mask),
                    Operator::Call { .. } => Some(Event::Call),
                    Operator::ReturnCall { .. } => Some(Event::Tail),
                    Operator::Return => Some(Event::Return),
                    Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                        code.writes += 1;
                        None
                    }
                    Operator::I32Add | Operator::I64Add => {
                        code.adds += 1;
                        None
                    }
                    Operator::Drop => {
                        code.drops += 1;
                        None
                    }
                    _ => None,
                };
                code.events.extend(event);
            }
        }
    }
    assert_eq!(bodies, 1);
    code
}

#[test]
fn value_arms_use_the_result_stack_before_the_join_is_saved() {
    for (bytes, carrier) in [
        (direct_result::<I32>(), ValType::I32),
        (direct_result::<I64>(), ValType::I64),
    ] {
        let code = inspect(bytes.bytes());
        assert_eq!(code.locals, 1);
        assert_eq!(code.writes, 1);
        assert_eq!(
            code.events,
            [
                Event::If(BlockType::Type(carrier)),
                Event::Else,
                Event::End,
                Event::Return
            ]
        );
    }
}

#[test]
fn shared_join_outputs_are_saved_after_one_selected_arm() {
    let code = inspect(shared_result().bytes());
    assert_eq!(code.locals, 1);
    assert_eq!(code.writes, 1);
    assert_eq!(code.adds, 3);
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::Store(0),
            Event::Else,
            Event::Store(0),
            Event::End,
            Event::Store(4),
            Event::Return,
        ]
    );
}

#[test]
fn selected_loads_follow_arm_stores_and_precede_continuation_stores() {
    assert_eq!(
        inspect(selected_memory().bytes()).events,
        [
            Event::Store(0),
            Event::If(BlockType::Type(ValType::I32)),
            Event::Store(4),
            Event::Load(0),
            Event::Else,
            Event::Store(4),
            Event::End,
            Event::Store(8),
            Event::Return,
        ]
    );
}

#[test]
fn unused_join_values_drop_reads_but_preserve_branch_effects() {
    let code = inspect(unused_result().bytes());
    assert_eq!(code.drops, 1);
    assert_eq!(code.locals, 0);
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Empty),
            Event::Store(0),
            Event::Call,
            Event::Else,
            Event::Store(0),
            Event::End,
            Event::Store(4),
            Event::Return,
        ]
    );
}

#[test]
fn prior_snapshots_survive_writes_in_either_arm_and_the_continuation() {
    assert_eq!(
        inspect(snapshots().bytes()).events,
        [
            Event::Load(0),
            Event::If(BlockType::Type(ValType::I32)),
            Event::Store(0),
            Event::Load(4),
            Event::Else,
            Event::Store(4),
            Event::Load(0),
            Event::End,
            Event::Store(0),
            Event::Return,
        ]
    );
}

#[test]
fn narrow_joins_normalize_at_observers_instead_of_arm_exits() {
    let code = inspect(narrow_result().bytes());
    assert_eq!(code.adds, 1);
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::Else,
            Event::End,
            Event::Store(0),
            Event::Mask,
            Event::Tail,
        ]
    );
    let code = inspect(predicate_result().bytes());
    assert_eq!(
        code.events
            .iter()
            .filter(|event| **event == Event::Mask)
            .count(),
        1
    );
}

#[test]
fn nested_joins_share_parent_values_without_repeating_arithmetic() {
    let code = inspect(nested_result().bytes());
    assert_eq!(code.locals, 2);
    assert_eq!(code.writes, 2);
    assert_eq!(code.adds, 5);
    assert_eq!(
        code.events
            .iter()
            .filter(|event| matches!(event, Event::If(_)))
            .count(),
        2
    );
}

#[test]
fn function_exits_inside_value_arms_keep_the_function_result_type() {
    let code = inspect(nested_fault().bytes());
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::If(BlockType::Empty),
            Event::Return,
            Event::End,
            Event::Load(0),
            Event::Else,
            Event::End,
            Event::Return,
        ]
    );
    let code = inspect(returning_arm().bytes());
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::Return,
            Event::Else,
            Event::End,
            Event::Store(0),
            Event::Return,
        ]
    );
    let code = inspect(tailing_arm().bytes());
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::Else,
            Event::Store(0),
            Event::Tail,
            Event::End,
            Event::Store(4),
            Event::Return,
        ]
    );
}

#[test]
fn conditional_values_choose_the_selected_i32_result() {
    let direct = direct_result::<I32>();
    let mut instance = direct.instantiate();
    assert_eq!(instance.call::<i32>((1, -2147483648, 17)), Ok(-2147483648));
    let mut instance = direct.instantiate();
    assert_eq!(instance.call::<i32>((0, -2147483648, 17)), Ok(17));
}

#[test]
fn conditional_values_preserve_i64_carriers() {
    let wide = direct_result::<I64>();
    let mut instance = wide.instantiate();
    assert_eq!(
        instance.call::<i64>((1, i64::MIN, i64::MAX)),
        Ok(-9223372036854775808)
    );
    let mut instance = wide.instantiate();
    assert_eq!(
        instance.call::<i64>((0, i64::MIN, i64::MAX)),
        Ok(9223372036854775807)
    );
}

#[test]
fn shared_conditional_results_execute_each_arm_once() {
    let shared = shared_result();
    let mut instance = shared.instantiate();
    assert_eq!(instance.call::<i32>((1, 2147483647)), Ok(0));
    assert_eq!(
        &instance.memory("state")[..10],
        &[1, 0, 0, 0, 0, 0, 0, 0x80, 0xa5, 0x5a]
    );
    let mut instance = shared.instantiate();
    assert_eq!(instance.call::<i32>((0, 4)), Ok(12));
    assert_eq!(
        &instance.memory("state")[..10],
        &[2, 0, 0, 0, 6, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn conditional_memory_values_evaluate_only_the_selected_load() {
    let selected = selected_memory();
    let mut instance = selected.instantiate();
    assert_eq!(instance.call::<i32>((1, 8)), Ok(11));
    assert_eq!(
        &instance.memory("state")[..14],
        &[1, 0, 0, 0, 2, 0, 0, 0, 4, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = selected.instantiate();
    assert!(instance.call::<i32>((1, 65536)).is_err());
    assert_eq!(
        &instance.memory("state")[..14],
        &[1, 0, 0, 0, 2, 0, 0, 0, 0x0b, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = selected.instantiate();
    assert_eq!(instance.call::<i32>((0, 65536)), Ok(11));
    assert_eq!(
        &instance.memory("state")[..14],
        &[1, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn unused_conditional_results_keep_selected_effects() {
    let unused = unused_result();
    let mut instance = unused.instantiate();
    assert_eq!(instance.call::<i32>(1), Ok(17));
    assert_eq!(
        instance.callbacks(),
        &[
            Call::new("receive", &[Value::I32(9)]).with_memories(&[MemoryBytes::new(
                "state",
                &[1, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
            )])
        ]
    );
    assert_eq!(
        &instance.memory("state")[..10],
        &[1, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = unused.instantiate();
    assert_eq!(instance.call::<i32>(0), Ok(17));
    assert!(instance.callbacks().is_empty());
    assert_eq!(
        &instance.memory("state")[..10],
        &[2, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn conditional_values_preserve_snapshots_across_stores() {
    let snapshots = snapshots();
    let mut instance = snapshots.instantiate();
    assert_eq!(instance.call::<i32>(1), Ok(19));
    assert_eq!(
        &instance.memory("state")[..10],
        &[0x0c, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = snapshots.instantiate();
    assert_eq!(instance.call::<i32>(0), Ok(21));
    assert_eq!(
        &instance.memory("state")[..10],
        &[0x0c, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn nested_conditional_faults_follow_the_selected_exit() {
    let fault = nested_fault();
    let mut instance = fault.instantiate();
    assert_eq!(
        instance.call::<i64>((1, 1, 65536)),
        Ok(-9223372036854775808)
    );
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = fault.instantiate();
    assert_eq!(instance.call::<i64>((1, 0, 0)), Ok(7));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = fault.instantiate();
    assert_eq!(instance.call::<i64>((0, 1, 65536)), Ok(11));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = fault.instantiate();
    assert!(instance.call::<i64>((1, 0, 65536)).is_err());
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn narrow_conditional_results_are_canonical_at_tail_boundaries() {
    let narrow = narrow_result();
    let mut instance = narrow.instantiate();
    assert_eq!(instance.call::<i64>((1, 255)), Ok(-9223372036854775808));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(0), Value::I32(0)])
            .with_memories(&[MemoryBytes::new("state", &[0, 0x5a])])]
    );
    assert_eq!(&instance.memory("state")[..2], &[0, 0x5a]);
    let mut instance = narrow.instantiate();
    assert_eq!(instance.call::<i64>((0, 255)), Ok(-9223372036854775808));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(7), Value::I32(7)])
            .with_memories(&[MemoryBytes::new("state", &[7, 0x5a])])]
    );
    assert_eq!(&instance.memory("state")[..2], &[7, 0x5a]);
}

#[test]
fn conditional_predicates_are_canonical_and_keep_snapshots() {
    let predicate = predicate_result();
    let mut instance = predicate.instantiate();
    assert_eq!(instance.call::<i32>((1, 1)), Ok(0));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = predicate.instantiate();
    assert_eq!(instance.call::<i32>((1, 0)), Ok(1));
    assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = predicate.instantiate();
    assert_eq!(instance.call::<i32>((0, 0)), Ok(0));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn nested_conditional_results_reach_their_join() {
    let nested = nested_result();
    let mut instance = nested.instantiate();
    assert_eq!(instance.call::<i32>((1, 1, 4)), Ok(11));
    assert_eq!(&instance.memory("state")[..6], &[5, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = nested.instantiate();
    assert_eq!(instance.call::<i32>((1, 0, 4)), Ok(12));
    assert_eq!(&instance.memory("state")[..6], &[5, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = nested.instantiate();
    assert_eq!(instance.call::<i32>((0, 1, 4)), Ok(13));
    assert_eq!(&instance.memory("state")[..6], &[5, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn returning_value_arms_skip_the_continuation() {
    let returning = returning_arm();
    let mut instance = returning.instantiate();
    assert_eq!(instance.call::<i64>(1), Ok(-9223372036854775808));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = returning.instantiate();
    assert_eq!(instance.call::<i64>(0), Ok(7));
    assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn tailing_value_arms_skip_the_continuation() {
    let tailing = tailing_arm();
    let mut instance = tailing.instantiate();
    assert_eq!(instance.call::<i64>(0), Ok(-9223372036854775808));
    assert_eq!(
        instance.callbacks(),
        &[
            Call::new("receive", &[Value::I32(9)]).with_memories(&[MemoryBytes::new(
                "state",
                &[1, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
            )])
        ]
    );
    assert_eq!(
        &instance.memory("state")[..10],
        &[1, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = tailing.instantiate();
    assert_eq!(instance.call::<i64>(1), Ok(11));
    assert!(instance.callbacks().is_empty());
    assert_eq!(
        &instance.memory("state")[..10],
        &[7, 0, 0, 0, 2, 0, 0, 0, 0xa5, 0x5a]
    );
}
