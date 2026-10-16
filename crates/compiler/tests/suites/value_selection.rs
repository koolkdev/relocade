use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};

use wasm86_compiler::{BlockBuilder, Mem, Type, Val, I1, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn selected_loads(body: &mut BlockBuilder<'_>, state: Mem) -> Val<I32> {
    let condition = body.parameter::<I1>(0).unwrap();
    let left = body.parameter::<I32>(1).unwrap();
    let right = body.parameter::<I32>(2).unwrap();
    let left = body.load_at::<I32>(state, left, 0).unwrap();
    let right = body.load_at::<I32>(state, right, 0).unwrap();
    condition.select(left, right)
}

fn eager_loads() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 9, 0, 0, 0]);
    fixture.function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let value = selected_loads(&mut body, state);
            body.return_(value)
        },
    )
}

fn lazy_loads() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 9, 0, 0, 0]);
    fixture.function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let left = body.parameter::<I32>(1)?;
            let right = body.parameter::<I32>(2)?;
            let value = body.if_value::<I32>(
                condition,
                |mut arm| {
                    let value = arm.load_at::<I32>(state, left, 0)?;
                    arm.yield_(value)
                },
                |mut arm| {
                    let value = arm.load_at::<I32>(state, right, 0)?;
                    arm.yield_(value)
                },
            )?;
            body.return_(value)
        },
    )
}

fn unused_selection() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    fixture.function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let _unused = selected_loads(&mut body, state);
            let value = body.value::<I32>(17)?;
            body.return_(value)
        },
    )
}

fn exit_selection() -> TestModule {
    let fixture = Fixture::new();
    fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let exit = body.parameter::<I1>(1)?;
            let value = body.parameter::<I32>(2)?;
            let selected = condition.select(value.add(1), value.add(2));
            body.if_(exit, |arm| arm.return_(selected))?;
            let value = body.value::<I32>(17)?;
            body.return_(value)
        },
    )
}

fn store_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let previous = body.load::<I32>(state, 0)?;
        let selected = condition.select(previous, 5);
        body.store::<I32>(state, 0, 9)?;
        let value = selected;
        body.return_(value)
    })
}

fn call_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0xa5, 0x5a]);
    let mutate = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut body| {
            body.store::<I32>(state, 0, 9)?;
            body.return_(11)
        })
        .unwrap();
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let previous = body.load::<I32>(state, 0)?;
        let selected = condition.select(previous, 5);
        let _unused = body.call::<I32>(mutate, &[])?;
        body.return_(selected)
    })
}

fn narrow_selection() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0x5a]);
    fixture.function(&[Type::I1, Type::I8], &[Type::I8], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let value = body.parameter::<I8>(1)?;
        let selected = condition.select(value.add(1), value.add(2));
        body.store(state, 0, &selected)?;
        let value = selected;
        body.return_(value)
    })
}

fn choice_comparisons() -> TestModule {
    Fixture::new().function(&[Type::I1, Type::I32], &[Type::I1; 4], |body| {
        let condition = body.parameter::<I1>(0)?;
        let unknown = body.parameter::<I32>(1)?;
        let choice = condition.select::<I32>(2, 3);
        let signed = condition.select::<I8>(0x80, 0);
        body.return_((
            choice.eq(2),
            choice.eq(3),
            signed.signed().lt(0),
            condition.select(2, unknown).eq(2),
        ))
    })
}

fn mapped_choices() -> (TestModule, MemoryBytes) {
    let mut fixture = Fixture::new();
    let values: Vec<_> = [0_u64, 0, 20, 30, 40]
        .into_iter()
        .flat_map(u64::to_le_bytes)
        .collect();
    let memory = fixture.memory("values", &values);
    let module = fixture.function(&[Type::I32, Type::I32], &[Type::I64], |mut body| {
        let first = body.parameter::<I32>(0)?.truncate::<I1>();
        let second = body.parameter::<I32>(1)?.truncate::<I1>();
        let choice = first.select::<I8>(0x102, second.select(3, 4));
        let mapped = body
            .map_constant_choices(
                choice,
                |body, bits| body.load::<I64>(memory, bits as u32 * 8).map(Some),
                |condition, first, second| condition.select(first, second),
            )?
            .expect("all choices are constants");
        body.return_(mapped)
    });
    (module, MemoryBytes::new("values", &values))
}

#[test]
fn mapped_choices_preserve_nested_conditions_and_logical_widths() {
    let (module, _) = mapped_choices();
    let mut instance = module.instantiate();
    for (input, expected) in [((0, 0), 40), ((0, 1), 30), ((1, 0), 20), ((2, 3), 30)] {
        assert_eq!(instance.call::<i64>(input).unwrap(), expected);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_mapped_choices_preserve_nested_conditions_and_logical_widths() {
    let (module, memory) = mapped_choices();
    for (input, expected) in [((0, 0), 40), ((0, 1), 30), ((1, 0), 20), ((2, 3), 30)] {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[Value::I32(input.0), Value::I32(input.1)])
                    .with_memories(std::slice::from_ref(&memory))
            ),
            Observation::returned(&[Value::I64(expected)])
                .with_memories(std::slice::from_ref(&memory))
        );
    }
}

#[test]
fn comparisons_recover_selected_conditions_and_keep_unknown_choices_dynamic() {
    let module = choice_comparisons();
    let mut instance = module.instantiate();
    for (input, expected) in [
        ((0, 3), (0, 1, 0, 0)),
        ((0, 2), (0, 1, 0, 1)),
        ((1, 3), (1, 0, 1, 1)),
    ] {
        assert_eq!(
            instance.call::<(i32, i32, i32, i32)>(input).unwrap(),
            expected
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_comparisons_recover_selected_conditions() {
    let module = choice_comparisons();
    for (input, expected) in [
        ((0, 3), [0, 1, 0, 0]),
        ((0, 2), [0, 1, 0, 1]),
        ((1, 3), [1, 0, 1, 1]),
    ] {
        assert_eq!(
            module.run_v8(&Input::call(
                "run",
                &[Value::I32(input.0), Value::I32(input.1)]
            )),
            Observation::returned(&expected.map(Value::I32))
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Load,
    Store,
    Call,
    Select,
    If,
    Else,
    End,
    Add,
    And,
    Return,
}

fn inspect(bytes: &[u8]) -> Vec<Vec<Event>> {
    Validator::new().validate_all(bytes).unwrap();
    let mut functions = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut events = Vec::new();
            let mut depth = 0;
            for operator in body.get_operators_reader().unwrap() {
                let event = match operator.unwrap() {
                    Operator::I32Load { .. } => Event::Load,
                    Operator::I32Store { .. } | Operator::I32Store8 { .. } => Event::Store,
                    Operator::Call { .. } => Event::Call,
                    Operator::Select => Event::Select,
                    Operator::If { .. } => {
                        depth += 1;
                        Event::If
                    }
                    Operator::Else => Event::Else,
                    Operator::End if depth > 0 => {
                        depth -= 1;
                        Event::End
                    }
                    Operator::I32Add => Event::Add,
                    Operator::I32And => Event::And,
                    Operator::Return => Event::Return,
                    _ => continue,
                };
                events.push(event);
            }
            functions.push(events);
        }
    }
    functions
}

#[test]
fn select_evaluates_both_loads_while_conditional_arms_are_lazy() {
    use Event::*;
    assert_eq!(
        inspect(eager_loads().bytes()),
        [vec![Load, Load, Select, Return]]
    );
    assert_eq!(
        inspect(lazy_loads().bytes()),
        [vec![If, Load, Else, Load, End, Return]]
    );
}

#[test]
fn unused_selection_omits_its_loads() {
    assert_eq!(inspect(unused_selection().bytes()), [vec![Event::Return]]);
}

#[test]
fn selection_used_only_by_an_exit_is_evaluated_inside_it() {
    use Event::*;
    assert_eq!(
        inspect(exit_selection().bytes()),
        [vec![If, Add, Add, Select, Return, End, Return]]
    );
}

#[test]
fn selected_loads_keep_their_snapshot_across_stores_and_calls() {
    use Event::*;
    assert_eq!(
        inspect(store_snapshot().bytes()),
        [vec![Load, Store, Select, Return]]
    );
    assert_eq!(
        inspect(call_snapshot().bytes()),
        [vec![Store, Return], vec![Load, Call, Select, Return]]
    );
}

#[test]
fn shared_narrow_selection_is_normalized_at_return_after_the_raw_store() {
    use Event::*;
    assert_eq!(
        inspect(narrow_selection().bytes()),
        [vec![Add, Add, Select, Store, And, Return]]
    );
}

#[test]
fn eager_and_lazy_selection_observe_their_load_evaluation_rules() {
    let eager = eager_loads();
    let lazy = lazy_loads();
    for arguments in [(1, 0, 65536), (0, 65536, 0)] {
        let mut instance = lazy.instantiate();
        assert_eq!(instance.call::<i32>(arguments).unwrap(), 7);
        assert_eq!(&instance.memory("state")[..8], &[7, 0, 0, 0, 9, 0, 0, 0]);
        assert!(instance.callbacks().is_empty());
    }
    let mut instance = eager.instantiate();
    assert_eq!(instance.call::<i32>((0, 0, 4)).unwrap(), 9);
    assert_eq!(&instance.memory("state")[..8], &[7, 0, 0, 0, 9, 0, 0, 0]);
    assert!(instance.callbacks().is_empty());
}

#[test]
fn unused_eager_selection_does_not_force_loads() {
    let mut instance = unused_selection().instantiate();
    assert_eq!(instance.call::<i32>((1, 65536, 65536)).unwrap(), 17);
    assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
    assert!(instance.callbacks().is_empty());
}

#[test]
fn selected_exit_values_follow_their_control_path() {
    let module = exit_selection();
    for (arguments, expected) in [((1, 1, 41), 42), ((0, 1, 41), 43), ((1, 0, 41), 17)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(arguments).unwrap(), expected);
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn selection_keeps_prior_snapshots_across_stores_and_calls() {
    for module in [store_snapshot(), call_snapshot()] {
        for (condition, expected) in [(1, 7), (0, 5)] {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>(condition).unwrap(), expected);
            assert_eq!(&instance.memory("state")[..6], &[9, 0, 0, 0, 0xa5, 0x5a]);
            assert!(instance.callbacks().is_empty());
        }
    }
}

#[test]
fn selected_narrow_values_are_canonical_at_stores() {
    let module = narrow_selection();
    for (condition, expected, bytes) in [(1, 0, [0, 0x5a]), (0, 1, [1, 0x5a])] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((condition, 255)).unwrap(), expected);
        assert_eq!(&instance.memory("state")[..2], bytes);
        assert!(instance.callbacks().is_empty());
    }
}

#[test]
fn selected_wide_values_preserve_i64_carriers() {
    let module = {
        let fixture = Fixture::new();
        fixture.function(&[Type::I1, Type::I64, Type::I64], &[Type::I64], |body| {
            let value = body
                .parameter::<I1>(0)?
                .select(body.parameter::<I64>(1)?, body.parameter::<I64>(2)?);
            body.return_(value)
        })
    };
    for (condition, expected) in [(1, i64::MIN), (0, i64::MAX)] {
        let mut instance = module.instantiate();
        assert_eq!(
            instance
                .call::<i64>((condition, i64::MIN, i64::MAX))
                .unwrap(),
            expected
        );
        assert!(instance.callbacks().is_empty());
    }
}
