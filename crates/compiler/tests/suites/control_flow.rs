use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, MemoryBytes, TestModule, Value};

use wasm86_compiler::{BuildError, Type, I1, I32, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn stores_and_tail() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]);
    let callback = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I64]),
        &[Value::I64(-1)],
    );
    fixture.function(&[Type::I1], &[Type::I64], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        body.store::<I32>(state, 0, 1)?;
        body.if_(&condition, |branch| {
            branch.return_(0x8000_0000_0000_0000u64)
        })?;
        body.store::<I32>(state, 4, 2)?;
        body.tail_call(callback, &[3.into()])
    })
}

fn alternative_stores() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let address = body.parameter::<I32>(1)?;
        let previous = body.load::<I32>(state, 0)?;
        body.if_else(
            condition,
            |mut arm| {
                let value = arm.load_at::<I32>(state, &address, 0)?;
                arm.store(state, 0, value.add(2))
            },
            |mut arm| arm.store::<I32>(state, 4, 11),
        )?;
        let result = previous.add(body.load::<I32>(state, 0)?);
        body.return_(result)
    })
}

#[test]
fn alternative_stores_preserve_the_prior_snapshot_and_selected_effects() {
    assert_eq!(
        inspect(alternative_stores().bytes()).events,
        [
            Event::Load(0),
            Event::If,
            Event::Load(0),
            Event::Store(0),
            Event::Else,
            Event::Store(4),
            Event::End,
            Event::Load(0),
            Event::Return,
        ]
    );
}

fn continuation_load(crosses_store: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let address = body.parameter::<I32>(1)?;
        let loaded = body.load_at::<I32>(state, &address, 0)?;
        body.if_(&condition, |branch| branch.return_(17))?;
        if crosses_store {
            body.store_at::<I32>(state, &address, 0, 9)?;
        }
        body.return_(loaded)
    })
}

fn conditional_result_load() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let address = body.parameter::<I32>(1)?;
        let loaded = body.load_at::<I32>(state, &address, 0)?;
        body.if_(&condition, |branch| branch.return_(&loaded))?;
        body.store::<I32>(state, 0, 9)?;
        body.return_(17)
    })
}

fn shared_snapshot(initial: &[u8]) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", initial);
    fixture.function(&[], &[Type::I32], |mut body| {
        let loaded = body.load::<I32>(state, 0)?;
        let shared = loaded.add(1);
        body.if_(loaded.eq(7), |branch| branch.return_(&shared))?;
        body.store::<I32>(state, 0, 9)?;
        body.return_(shared)
    })
}

fn sequential_exits(initial: &[u8]) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", initial);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let first = body.parameter::<I1>(0)?;
        body.if_(&first, |branch| branch.return_(11))?;
        let address = body.parameter::<I32>(1)?;
        let loaded = body.load_at::<I32>(state, &address, 0)?;
        body.if_(loaded.eq(7), |branch| branch.return_(22))?;
        body.store::<I32>(state, 4, 9)?;
        body.return_(33)
    })
}

fn narrow_condition_and_results() -> TestModule {
    let fixture = Fixture::new();
    fixture.function(&[Type::I1, Type::I8], &[Type::I8], |mut body| {
        let condition = body.parameter::<I1>(0)?.add(1);
        let value = body.parameter::<I8>(1)?.add(1);
        body.if_(&condition, |branch| branch.return_(&value))?;
        body.return_(value.add(1))
    })
}

fn branch_load_after_store() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 9, 0, 0, 0, 11, 0, 0, 0]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let address = body.parameter::<I32>(1)?;
        body.store::<I32>(state, 0, 1)?;
        body.if_(&condition, |mut branch| {
            branch.store::<I32>(state, 4, 2)?;
            let loaded = branch.load_at::<I32>(state, &address, 0)?;
            branch.return_(&loaded)
        })?;
        body.store::<I32>(state, 8, 3)?;
        body.return_(17)
    })
}

fn fallthrough_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let before = body.load_at::<I32>(state, 0, 0)?;
        body.if_(&condition, |mut branch| {
            branch.store_at::<I32>(state, 0, 0, 9)?;
            Ok(())
        })?;
        let after = body.load_at::<I32>(state, 0, 0)?;
        body.return_(before.add(&after))
    })
}

fn shared_pure_value() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?;
        let mut pure = None;
        body.if_(&condition, |mut branch| {
            let value = input.add(1);
            branch.store(state, 0, &value)?;
            pure = Some(value);
            Ok(())
        })?;
        body.return_(pure.unwrap())
    })
}

fn nested_tail() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 9, 0, 0, 0, 0xa5, 0x5a]);
    let callback = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I64]),
        &[Value::I64(-9223372036854775808)],
    );
    fixture.function(&[Type::I1; 2], &[Type::I64], |mut body| {
        let outer = body.parameter::<I1>(0)?;
        let inner = body.parameter::<I1>(1)?;
        body.if_(&outer, |mut branch| {
            branch.store::<I32>(state, 0, 1)?;
            branch.if_(&inner, |inner| inner.tail_call(callback, &[11.into()]))?;
            branch.store::<I32>(state, 4, 2)?;
            Ok(())
        })?;
        body.store::<I32>(state, 8, 3)?;
        body.return_(17)
    })
}

#[derive(Debug, PartialEq)]
enum Event {
    Else,
    If,
    End,
    Load(u64),
    Store(u64),
    Return,
    Tail,
}

#[derive(Default)]
struct Code {
    events: Vec<Event>,
    additions: usize,
    masks: usize,
    writes: usize,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            bodies += 1;
            let mut depth = 0;
            let mut operators = body.get_operators_reader().unwrap();
            while !operators.eof() {
                let event = match operators.read().unwrap() {
                    Operator::If { .. } => {
                        depth += 1;
                        Event::If
                    }
                    Operator::End if depth > 0 => {
                        depth -= 1;
                        Event::End
                    }
                    Operator::Else => Event::Else,
                    Operator::I32Load { memarg } => Event::Load(memarg.offset),
                    Operator::I32Store { memarg } => Event::Store(memarg.offset),
                    Operator::Return => Event::Return,
                    Operator::ReturnCall { .. } => Event::Tail,
                    Operator::I32Add | Operator::I64Add => {
                        code.additions += 1;
                        continue;
                    }
                    Operator::I32And => {
                        code.masks += 1;
                        continue;
                    }
                    Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                        code.writes += 1;
                        continue;
                    }
                    Operator::Call { .. } => panic!("the continuation must use a tail call"),
                    _ => continue,
                };
                code.events.push(event);
            }
        }
    }
    assert_eq!(bodies, 1);
    code
}

#[test]
fn conditional_return_skips_later_stores_and_the_tail_call() {
    assert_eq!(
        inspect(stores_and_tail().bytes()).events,
        [
            Event::Store(0),
            Event::If,
            Event::Return,
            Event::End,
            Event::Store(4),
            Event::Tail
        ]
    );
}

#[test]
fn loads_used_on_only_one_path_stay_on_that_path() {
    let continuing = inspect(continuation_load(false).bytes());
    assert_eq!(
        continuing.events,
        [
            Event::If,
            Event::Return,
            Event::End,
            Event::Load(0),
            Event::Return
        ]
    );
    assert_eq!(continuing.writes, 0);
    let exiting = inspect(conditional_result_load().bytes());
    assert_eq!(
        exiting.events,
        [
            Event::If,
            Event::Load(0),
            Event::Return,
            Event::End,
            Event::Store(0),
            Event::Return
        ]
    );
    assert_eq!(exiting.writes, 0);
}

#[test]
fn overlapping_stores_preserve_a_snapshot_across_the_guard() {
    let captured = inspect(continuation_load(true).bytes());
    assert_eq!(
        captured.events,
        [
            Event::Load(0),
            Event::If,
            Event::Return,
            Event::End,
            Event::Store(0),
            Event::Return
        ]
    );
    assert_eq!(captured.writes, 1);
    let shared = inspect(shared_snapshot(&[]).bytes());
    assert_eq!(
        shared.events,
        [
            Event::Load(0),
            Event::If,
            Event::Return,
            Event::End,
            Event::Store(0),
            Event::Return
        ]
    );
    // Both returns compute the cheap addition from the same captured load.
    assert_eq!(shared.additions, 2);
}

#[test]
fn later_exit_conditions_are_evaluated_only_after_earlier_guards() {
    assert_eq!(
        inspect(sequential_exits(&[]).bytes()).events,
        [
            Event::If,
            Event::Return,
            Event::End,
            Event::Load(0),
            Event::If,
            Event::Return,
            Event::End,
            Event::Store(4),
            Event::Return
        ]
    );
}

#[test]
fn narrow_conditions_and_each_return_observe_their_logical_bits() {
    let code = inspect(narrow_condition_and_results().bytes());
    assert_eq!(
        code.events,
        [Event::If, Event::Return, Event::End, Event::Return]
    );
    assert_eq!(code.masks, 3);
}

#[test]
fn branch_loads_and_stores_remain_inside_the_selected_arm() {
    assert_eq!(
        inspect(branch_load_after_store().bytes()).events,
        [
            Event::Store(0),
            Event::If,
            Event::Store(4),
            Event::Load(0),
            Event::Return,
            Event::End,
            Event::Store(8),
            Event::Return
        ]
    );
    assert_eq!(
        inspect(nested_tail().bytes()).events,
        [
            Event::If,
            Event::Store(0),
            Event::If,
            Event::Tail,
            Event::End,
            Event::Store(4),
            Event::End,
            Event::Store(8),
            Event::Return
        ]
    );
}

#[test]
fn falling_through_a_branch_preserves_snapshots_and_shared_values() {
    let snapshot = inspect(fallthrough_snapshot().bytes());
    assert_eq!(
        snapshot.events,
        [
            Event::Load(0),
            Event::If,
            Event::Store(0),
            Event::End,
            Event::Load(0),
            Event::Return
        ]
    );
    assert_eq!(snapshot.writes, 1);
    let shared = inspect(shared_pure_value().bytes());
    assert_eq!(
        shared.events,
        [Event::If, Event::Store(0), Event::End, Event::Return]
    );
    // The child store and parent return each compute the addition without a local.
    assert_eq!((shared.additions, shared.writes), (2, 0));
}

#[test]
fn child_load_dependencies_are_not_visible_to_parent_or_sibling_consumers() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[]);
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let mut escaped = None;
        body.if_(&condition, |mut branch| {
            let loaded = branch.load::<I32>(state, 0)?;
            branch.store(state, 12, &loaded)?;
            escaped = Some(loaded);
            Ok(())
        })?;
        let loaded = escaped.unwrap();
        assert!(matches!(
            body.value(loaded.add(1)),
            Err(BuildError::OutOfScope)
        ));
        body.if_(&condition, |mut sibling| {
            assert_eq!(
                sibling.store(state, 8, &loaded),
                Err(BuildError::OutOfScope)
            );
            Ok(())
        })?;
        body.return_(7)
    });
    Validator::new().validate_all(module.bytes()).unwrap();
}

#[test]
fn alternative_stores_execute_only_the_selected_arm() {
    let alternatives = alternative_stores();
    for (condition, address, expected_result, expected_memory) in [
        (1, 0, Some(16), &[9, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]),
        (0, 65536, Some(14), &[7, 0, 0, 0, 0x0b, 0, 0, 0, 0xa5, 0x5a]),
        (1, 65536, None, &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]),
    ] {
        let mut instance = alternatives.instantiate();
        assert_eq!(
            instance.call::<i32>((condition, address)).ok(),
            expected_result
        );
        assert_eq!(&instance.memory("state")[..10], expected_memory);
    }
}

#[test]
fn returning_branches_skip_later_stores_and_tail_calls() {
    let tail = stores_and_tail();
    let mut instance = tail.instantiate();
    assert_eq!(instance.call::<i64>(1), Ok(-9223372036854775808));
    assert!(instance.callbacks().is_empty());
    assert_eq!(
        &instance.memory("state")[..10],
        &[1, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = tail.instantiate();
    assert_eq!(instance.call::<i64>(0), Ok(-1));
    assert_eq!(
        instance.callbacks(),
        &[
            Call::new("receive", &[Value::I32(3)]).with_memories(&[MemoryBytes::new(
                "state",
                &[1, 0, 0, 0, 2, 0, 0, 0, 0xa5, 0x5a]
            )])
        ]
    );
    assert_eq!(
        &instance.memory("state")[..10],
        &[1, 0, 0, 0, 2, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn continuation_loads_execute_only_after_the_guard() {
    let continuing = continuation_load(false);
    let mut instance = continuing.instantiate();
    assert_eq!(instance.call::<i32>((1, 65536)), Ok(17));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = continuing.instantiate();
    assert!(instance.call::<i32>((0, 65536)).is_err());
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = continuing.instantiate();
    assert_eq!(instance.call::<i32>((0, 0)), Ok(7));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn conditional_result_loads_follow_their_return_path() {
    let exiting = conditional_result_load();
    let mut instance = exiting.instantiate();
    assert_eq!(instance.call::<i32>((0, 65536)), Ok(17));
    assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = exiting.instantiate();
    assert!(instance.call::<i32>((1, 65536)).is_err());
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = exiting.instantiate();
    assert_eq!(instance.call::<i32>((1, 0)), Ok(7));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn captured_loads_precede_guards_that_can_write_the_snapshot() {
    let captured = continuation_load(true);
    // Preserving this earlier read across the overlapping write forces its capture before the guard.
    let mut instance = captured.instantiate();
    assert!(instance.call::<i32>((1, 65536)).is_err());
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = captured.instantiate();
    assert_eq!(instance.call::<i32>((1, 0)), Ok(17));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = captured.instantiate();
    assert_eq!(instance.call::<i32>((0, 0)), Ok(7));
    assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn shared_snapshots_survive_conditional_writes() {
    let mut instance = shared_snapshot(&[7, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(8));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = shared_snapshot(&[5, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(6));
    assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn sequential_exits_preserve_effect_order() {
    let mut instance = sequential_exits(&[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert_eq!(instance.call::<i32>((1, 65536)), Ok(11));
    assert_eq!(
        &instance.memory("state")[..10],
        &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = sequential_exits(&[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert!(instance.call::<i32>((0, 65536)).is_err());
    assert_eq!(
        &instance.memory("state")[..10],
        &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = sequential_exits(&[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert_eq!(instance.call::<i32>((0, 0)), Ok(22));
    assert_eq!(
        &instance.memory("state")[..10],
        &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = sequential_exits(&[5, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]).instantiate();
    assert_eq!(instance.call::<i32>((0, 0)), Ok(33));
    assert_eq!(
        &instance.memory("state")[..10],
        &[5, 0, 0, 0, 9, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn constant_guards_keep_only_reachable_effects() {
    for (taken, result, expected_memory) in [
        (false, 33, &[1, 0, 0, 0, 2, 0, 0, 0, 0xa5, 0x5a]),
        (true, 17, &[1, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]),
    ] {
        let mut fixture = Fixture::new();
        let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 0xa5, 0x5a]);
        let module = fixture.function(&[], &[Type::I32], |mut body| {
            body.store::<I32>(state, 0, 1)?;
            body.if_(taken, |branch| branch.return_(17))?;
            body.store::<I32>(state, 4, 2)?;
            body.return_(33)
        });

        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(()), Ok(result));
        assert_eq!(&instance.memory("state")[..10], expected_memory);
    }
}

#[test]
fn branch_loads_observe_prior_stores_and_trap_in_order() {
    let arm = branch_load_after_store();
    let mut instance = arm.instantiate();
    assert_eq!(instance.call::<i32>((0, 65536)), Ok(17));
    assert_eq!(
        &instance.memory("state")[..16],
        &[1, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0x0b, 0, 0, 0]
    );
    let mut instance = arm.instantiate();
    assert!(instance.call::<i32>((1, 65536)).is_err());
    assert_eq!(
        &instance.memory("state")[..16],
        &[1, 0, 0, 0, 2, 0, 0, 0, 9, 0, 0, 0, 0x0b, 0, 0, 0]
    );
    let mut instance = arm.instantiate();
    assert_eq!(instance.call::<i32>((1, 12)), Ok(11));
    assert_eq!(
        &instance.memory("state")[..16],
        &[1, 0, 0, 0, 2, 0, 0, 0, 9, 0, 0, 0, 0x0b, 0, 0, 0]
    );
}

#[test]
fn fallthrough_reads_observe_conditional_stores() {
    let fallthrough = fallthrough_snapshot();
    let mut instance = fallthrough.instantiate();
    assert_eq!(instance.call::<i32>(0), Ok(14));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = fallthrough.instantiate();
    assert_eq!(instance.call::<i32>(1), Ok(16));
    assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn pure_values_can_be_shared_across_conditional_stores() {
    let pure = shared_pure_value();
    let mut instance = pure.instantiate();
    assert_eq!(instance.call::<i32>((0, 41)), Ok(42));
    assert_eq!(&instance.memory("state")[..6], &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mut instance = pure.instantiate();
    assert_eq!(instance.call::<i32>((1, 41)), Ok(42));
    assert_eq!(&instance.memory("state")[..6], &[0x2a, 0, 0, 0, 0xa5, 0x5a]);
}

#[test]
fn nested_tail_calls_skip_outer_continuations() {
    let nested = nested_tail();
    let mut instance = nested.instantiate();
    assert_eq!(instance.call::<i64>((0, 1)), Ok(17));
    assert!(instance.callbacks().is_empty());
    assert_eq!(
        &instance.memory("state")[..14],
        &[7, 0, 0, 0, 5, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = nested.instantiate();
    assert_eq!(instance.call::<i64>((1, 0)), Ok(17));
    assert!(instance.callbacks().is_empty());
    assert_eq!(
        &instance.memory("state")[..14],
        &[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a]
    );
    let mut instance = nested.instantiate();
    assert_eq!(instance.call::<i64>((1, 1)), Ok(-9223372036854775808));
    assert_eq!(
        instance.callbacks(),
        &[
            Call::new("receive", &[Value::I32(11)]).with_memories(&[MemoryBytes::new(
                "state",
                &[1, 0, 0, 0, 5, 0, 0, 0, 9, 0, 0, 0, 0xa5, 0x5a]
            )])
        ]
    );
    assert_eq!(
        &instance.memory("state")[..14],
        &[1, 0, 0, 0, 5, 0, 0, 0, 9, 0, 0, 0, 0xa5, 0x5a]
    );
}

#[test]
fn narrow_conditions_and_results_stay_canonical_at_runtime() {
    let narrow = narrow_condition_and_results();
    let mut instance = narrow.instantiate();
    assert_eq!(instance.call::<i32>((0, 255)), Ok(0));
    let mut instance = narrow.instantiate();
    assert_eq!(instance.call::<i32>((1, 255)), Ok(1));
    let mut instance = narrow.instantiate();
    assert_eq!(instance.call::<i32>((1, 254)), Ok(0));
}
