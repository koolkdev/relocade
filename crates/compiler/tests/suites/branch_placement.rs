use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, MemoryBytes, TestModule, Value};

use wasm86_compiler::{Type, I1, I32};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

#[path = "branch_placement/guarded.rs"]
mod guarded;
#[path = "branch_placement/recomputation.rs"]
mod recomputation;

fn exclusive_switch_arms() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    fixture.function(
        &[Type::I32, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let selector = body.parameter::<I32>(0)?;
            let input = body.parameter::<I32>(1)?;
            let address = body.parameter::<I32>(2)?;
            let shared = input.xor(0x8000_0000u32);
            let result =
                body.switch_value::<I32, _>(selector, &[10, 11, 12, 13], |mut arm, key| {
                    if let Some(key) = key {
                        arm.store_at(state, &address, 0, &shared)?;
                        arm.store::<I32>(state, 4, key)?;
                        arm.yield_(&shared)
                    } else {
                        arm.yield_(17)
                    }
                })?;
            body.return_(result)
        },
    )
}

fn nested_uses_in_switch_arms() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    fixture.function(
        &[Type::I32, Type::I1, Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let selector = body.parameter::<I32>(0)?;
            let first = body.parameter::<I1>(1)?;
            let second = body.parameter::<I1>(2)?;
            let shared = body.parameter::<I32>(3)?.xor(1);
            let address = body.parameter::<I32>(4)?;
            let result =
                body.switch_value::<I32, _>(selector, &[10, 11, 12, 13], |mut arm, key| {
                    if let Some(key) = key {
                        arm.if_(&first, |mut inner| {
                            inner.store_at(state, &address, 0, &shared)
                        })?;
                        arm.if_(&second, |mut inner| {
                            inner.store_at(state, &address, 4, &shared)
                        })?;
                        arm.store::<I32>(state, 8, key)?;
                        arm.yield_(&shared)
                    } else {
                        arm.yield_(17)
                    }
                })?;
            body.return_(result)
        },
    )
}

fn dependency_used_after_the_join() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let base = body.parameter::<I32>(1)?.add(1);
        let branch_value = base.xor(7);
        body.if_else(
            condition,
            |mut arm| arm.store(state, 0, &branch_value),
            |mut arm| arm.store(state, 4, &branch_value),
        )?;
        body.return_(base)
    })
}

fn sequential_controls() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let first = body.parameter::<I1>(0)?;
            let second = body.parameter::<I1>(1)?;
            let shared = body.parameter::<I32>(2)?.xor(1);
            body.if_(first, |mut arm| arm.store(state, 0, &shared))?;
            body.if_(second, |mut arm| arm.store(state, 4, &shared))?;
            body.return_(17)
        },
    )
}

fn sequential_controls_inside_an_exclusive_arm() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    fixture.function(
        &[Type::I1, Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let outer = body.parameter::<I1>(0)?;
            let first = body.parameter::<I1>(1)?;
            let second = body.parameter::<I1>(2)?;
            let shared = body.parameter::<I32>(3)?.xor(1);
            body.if_else(
                outer,
                |mut arm| {
                    arm.if_(first, |mut inner| inner.store(state, 0, &shared))?;
                    arm.if_(second, |mut inner| inner.store(state, 4, &shared))
                },
                |mut arm| arm.store(state, 8, &shared),
            )?;
            body.return_(17)
        },
    )
}

fn shared_address_needed_by_a_parent_load() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let address = body.parameter::<I32>(1)?.add(4);
        let previous = body.load_at::<I32>(state, &address, 0)?;
        body.if_else(
            condition,
            |mut arm| {
                arm.store_at::<I32>(state, &address, 0, 13)?;
                arm.store(state, 0, &previous)
            },
            |mut arm| {
                arm.store_at::<I32>(state, &address, 0, 15)?;
                arm.store(state, 0, &previous)
            },
        )?;
        body.return_(17)
    })
}

#[derive(Clone, Copy)]
enum Snapshot {
    Load,
    Call,
    Join,
}

fn snapshot_across_arm_writes(source: Snapshot) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]);
    let receive = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I32]),
        &[Value::I32(7)],
    );
    fixture.function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let destination = body.parameter::<I1>(0)?;
        let choose_load = body.parameter::<I1>(1)?;
        let previous = match source {
            Snapshot::Load => body.load::<I32>(state, 0)?,
            Snapshot::Call => body.call::<I32>(receive, &[9.into()])?,
            Snapshot::Join => body.if_value::<I32>(
                choose_load,
                |mut arm| {
                    let previous = arm.load::<I32>(state, 0)?;
                    arm.yield_(previous)
                },
                |arm| arm.yield_(11),
            )?,
        };
        let derived = previous.xor(1);
        body.if_else(
            destination,
            |mut arm| {
                arm.store::<I32>(state, 0, 13)?;
                arm.store(state, 4, &derived)
            },
            |mut arm| {
                arm.store::<I32>(state, 0, 15)?;
                arm.store(state, 4, &derived)
            },
        )?;
        body.return_(17)
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    If,
    Else,
    End,
    Table,
    BlockEnd,
    Add,
    And,
    Mul,
    Xor,
    Constant(i32),
    Compare,
    ZeroTest,
    LocalWrite,
    Load,
    Store,
    Call,
}

fn inspect(bytes: &[u8]) -> Vec<Event> {
    Validator::new().validate_all(bytes).unwrap();
    let mut imports = 0;
    let mut run = None;
    let mut body_index = 0;
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
                        run = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                if Some(imports + body_index) != run {
                    body_index += 1;
                    continue;
                }
                let mut events = Vec::new();
                let mut controls = Vec::new();
                for operator in body.get_operators_reader().unwrap() {
                    let event = match operator.unwrap() {
                        Operator::If { .. } => {
                            controls.push(true);
                            Event::If
                        }
                        Operator::Block { .. } => {
                            controls.push(false);
                            continue;
                        }
                        Operator::Else => Event::Else,
                        Operator::End => match controls.pop() {
                            Some(true) => Event::End,
                            Some(false) => Event::BlockEnd,
                            None => continue,
                        },
                        Operator::BrTable { .. } => Event::Table,
                        Operator::I32Add => Event::Add,
                        Operator::I32And => Event::And,
                        Operator::I32Mul => Event::Mul,
                        Operator::I32Const { value } => Event::Constant(value),
                        Operator::I32Xor => Event::Xor,
                        Operator::I32GeU => Event::Compare,
                        Operator::I32Eqz => Event::ZeroTest,
                        Operator::LocalSet { .. } | Operator::LocalTee { .. } => Event::LocalWrite,
                        Operator::I32Load { .. } => Event::Load,
                        Operator::I32Store { .. } => Event::Store,
                        Operator::Call { .. } => Event::Call,
                        _ => continue,
                    };
                    events.push(event);
                }
                return events;
            }
            _ => {}
        }
    }
    panic!("the run export has no body");
}

// These fixtures contain only fallthrough If/Else controls. Count arithmetic on
// possible paths so two sequential inner arms cannot masquerade as exclusivity.
fn xor_counts_on_paths(events: &[Event], cursor: &mut usize) -> Vec<usize> {
    let mut counts = vec![0];
    while *cursor < events.len() {
        let event = events[*cursor];
        *cursor += 1;
        match event {
            Event::If => {
                let mut arms = xor_counts_on_paths(events, cursor);
                if events[*cursor - 1] == Event::Else {
                    arms.extend(xor_counts_on_paths(events, cursor));
                } else {
                    arms.push(0);
                }
                counts = counts
                    .iter()
                    .flat_map(|prefix| arms.iter().map(move |arm| prefix + arm))
                    .collect();
            }
            Event::Else | Event::End => return counts,
            Event::Xor => counts.iter_mut().for_each(|count| *count += 1),
            Event::Table => panic!("path counter is only for the If/Else fixtures"),
            _ => {}
        }
    }
    counts
}

#[test]
fn pure_values_shared_within_exclusive_cases_stay_after_dispatch() {
    let events = inspect(exclusive_switch_arms().bytes());
    let dispatch = events
        .iter()
        .position(|event| *event == Event::Table)
        .unwrap();
    assert!(!events[..dispatch].contains(&Event::Xor));
    assert_eq!(
        events.iter().filter(|event| **event == Event::Xor).count(),
        4
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == Event::Store)
            .count(),
        8
    );
}

#[test]
fn nested_uses_share_one_capture_inside_each_selected_switch_arm() {
    let events = inspect(nested_uses_in_switch_arms().bytes());
    let dispatch = events
        .iter()
        .position(|event| *event == Event::Table)
        .unwrap();
    assert!(!events[..dispatch].contains(&Event::Xor));
    // Each switch label closes its dispatch block before its case body. The
    // nested controls here are If/Else, so their ends do not divide the cases.
    let arms: Vec<_> = events[dispatch + 1..]
        .split(|event| *event == Event::BlockEnd)
        .filter(|arm| arm.contains(&Event::If))
        .collect();
    assert_eq!(arms.len(), 4);
    for arm in arms {
        assert_eq!(xor_counts_on_paths(arm, &mut 0), [1, 1, 1, 1], "{arm:?}");
    }
}

#[test]
fn a_shared_dependency_needed_after_the_join_keeps_its_capture() {
    let events = inspect(dependency_used_after_the_join().bytes());
    let branch = events.iter().position(|event| *event == Event::If).unwrap();
    assert_eq!(
        events.iter().filter(|event| **event == Event::Add).count(),
        1
    );
    assert!(events[..branch].contains(&Event::Add));
    assert!(!events[..branch].contains(&Event::Xor));
    assert_eq!(xor_counts_on_paths(&events, &mut 0), [1, 1]);
}

#[test]
fn two_sequential_controls_each_compute_their_shared_cheap_value() {
    let events = inspect(sequential_controls().bytes());
    let branch = events.iter().position(|event| *event == Event::If).unwrap();
    assert!(!events[..branch].contains(&Event::Xor));
    assert_eq!(
        events.iter().filter(|&&event| event == Event::Xor).count(),
        2
    );
    assert_eq!(xor_counts_on_paths(&events, &mut 0), [2, 1, 1, 0]);
}

#[test]
fn nested_sequential_uses_stay_inside_their_outer_conditional_arm() {
    let events = inspect(sequential_controls_inside_an_exclusive_arm().bytes());
    let branch = events.iter().position(|event| *event == Event::If).unwrap();
    assert!(!events[..branch].contains(&Event::Xor));
    assert_eq!(xor_counts_on_paths(&events, &mut 0).iter().max(), Some(&1));
}

#[test]
fn a_shared_address_is_available_when_the_parent_captures_its_load() {
    let events = inspect(shared_address_needed_by_a_parent_load().bytes());
    assert_eq!(
        events.iter().filter(|event| **event == Event::Add).count(),
        1
    );
    let address = events
        .iter()
        .position(|event| *event == Event::Add)
        .unwrap();
    let read = events
        .iter()
        .position(|event| *event == Event::Load)
        .unwrap();
    let branch = events.iter().position(|event| *event == Event::If).unwrap();
    assert!(address < read && read < branch, "{events:?}");
}

#[test]
fn load_call_and_join_inputs_keep_their_snapshot_before_arm_writes() {
    for source in [Snapshot::Load, Snapshot::Call, Snapshot::Join] {
        let events = inspect(snapshot_across_arm_writes(source).bytes());
        let input = match source {
            Snapshot::Call => Event::Call,
            _ => Event::Load,
        };
        assert_eq!(events.iter().filter(|event| **event == input).count(), 1);
        let read = events.iter().position(|event| *event == input).unwrap();
        let write = events
            .iter()
            .position(|event| *event == Event::Store)
            .unwrap();
        assert!(read < write, "{events:?}");
        assert_eq!(
            events.iter().filter(|event| **event == Event::Xor).count(),
            2
        );
        assert_eq!(xor_counts_on_paths(&events, &mut 0).iter().max(), Some(&1));
    }
}

#[test]
fn exclusive_switch_arms_execute_only_selected_shared_values() {
    let exclusive = exclusive_switch_arms();
    for (selector, state) in [
        (10, &[7, 0, 0, 0x80, 0x0a, 0, 0, 0, 6, 0, 0, 0]),
        (11, &[7, 0, 0, 0x80, 0x0b, 0, 0, 0, 6, 0, 0, 0]),
        (12, &[7, 0, 0, 0x80, 0x0c, 0, 0, 0, 6, 0, 0, 0]),
        (13, &[7, 0, 0, 0x80, 0x0d, 0, 0, 0, 6, 0, 0, 0]),
    ] {
        let mut instance = exclusive.instantiate();
        assert_eq!(instance.call::<i32>((selector, 7, 0)), Ok(-2147483641));
        assert_eq!(&instance.memory("state")[..12], state);
    }
    let mut instance = exclusive.instantiate();
    assert_eq!(instance.call::<i32>((10, -2147483648, 0)), Ok(0));
    assert_eq!(
        &instance.memory("state")[..12],
        &[0, 0, 0, 0, 0x0a, 0, 0, 0, 6, 0, 0, 0]
    );
    let mut instance = exclusive.instantiate();
    assert_eq!(instance.call::<i32>((9, 7, 65536)), Ok(17));
    assert_eq!(
        &instance.memory("state")[..12],
        &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]
    );
    let mut instance = exclusive.instantiate();
    assert!(instance.call::<i32>((10, 7, 65536)).is_err());
    assert_eq!(
        &instance.memory("state")[..12],
        &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]
    );
}

#[test]
fn nested_switch_uses_execute_only_the_reached_dependencies() {
    let nested_switch = nested_uses_in_switch_arms();
    for (selector, first, second, state) in [
        (10, 1, 1, &[8, 0, 0, 0, 8, 0, 0, 0, 0x0a, 0, 0, 0]),
        (11, 1, 0, &[8, 0, 0, 0, 5, 0, 0, 0, 0x0b, 0, 0, 0]),
        (12, 0, 1, &[7, 0, 0, 0, 8, 0, 0, 0, 0x0c, 0, 0, 0]),
        (13, 0, 0, &[7, 0, 0, 0, 5, 0, 0, 0, 0x0d, 0, 0, 0]),
    ] {
        let mut instance = nested_switch.instantiate();
        assert_eq!(instance.call::<i32>((selector, first, second, 9, 0)), Ok(8));
        assert_eq!(&instance.memory("state")[..12], state);
    }
    for (selector, first, second, expected_result, expected_memory) in [
        (9, 1, 1, Some(17), &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]),
        (10, 0, 0, Some(8), &[7, 0, 0, 0, 5, 0, 0, 0, 0x0a, 0, 0, 0]),
        (10, 1, 0, None, &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]),
    ] {
        let mut instance = nested_switch.instantiate();
        assert_eq!(
            instance
                .call::<i32>((selector, first, second, 9, 65536))
                .ok(),
            expected_result
        );
        assert_eq!(&instance.memory("state")[..12], expected_memory);
    }
}

#[test]
fn dependencies_used_after_a_join_survive_arm_writes() {
    let dependency = dependency_used_after_the_join();
    for (condition, state) in [
        (1, &[0x0d, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]),
        (0, &[7, 0, 0, 0, 0x0d, 0, 0, 0, 6, 0, 0, 0]),
    ] {
        let mut instance = dependency.instantiate();
        assert_eq!(instance.call::<i32>((condition, 9)), Ok(10));
        assert_eq!(&instance.memory("state")[..12], state);
    }
}

#[test]
fn sequential_controls_share_values_without_forcing_earlier_uses() {
    let sequential = sequential_controls();
    for (first, second, state) in [
        (1, 1, &[6, 0, 0, 0, 6, 0, 0, 0, 6, 0, 0, 0]),
        (0, 1, &[7, 0, 0, 0, 6, 0, 0, 0, 6, 0, 0, 0]),
        (0, 0, &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]),
    ] {
        let mut instance = sequential.instantiate();
        assert_eq!(instance.call::<i32>((first, second, 7)), Ok(17));
        assert_eq!(&instance.memory("state")[..12], state);
    }
}

#[test]
fn sequential_controls_inside_exclusive_arms_preserve_placement() {
    let nested = sequential_controls_inside_an_exclusive_arm();
    for (outer, first, second, state) in [
        (1, 1, 1, &[8, 0, 0, 0, 8, 0, 0, 0, 6, 0, 0, 0]),
        (1, 1, 0, &[8, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]),
        (0, 1, 1, &[7, 0, 0, 0, 5, 0, 0, 0, 8, 0, 0, 0]),
    ] {
        let mut instance = nested.instantiate();
        assert_eq!(instance.call::<i32>((outer, first, second, 9)), Ok(17));
        assert_eq!(&instance.memory("state")[..12], state);
    }
}

#[test]
fn shared_addresses_remain_available_for_parent_loads() {
    let address = shared_address_needed_by_a_parent_load();
    for (condition, input, expected_result, expected_memory) in [
        (1, 0, Some(17), &[5, 0, 0, 0, 0x0d, 0, 0, 0, 6, 0, 0, 0]),
        (0, 0, Some(17), &[5, 0, 0, 0, 0x0f, 0, 0, 0, 6, 0, 0, 0]),
        (1, 4, Some(17), &[6, 0, 0, 0, 5, 0, 0, 0, 0x0d, 0, 0, 0]),
        (0, 65532, None, &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0]),
    ] {
        let mut instance = address.instantiate();
        assert_eq!(
            instance.call::<i32>((condition, input)).ok(),
            expected_result
        );
        assert_eq!(&instance.memory("state")[..12], expected_memory);
    }
}

#[test]
fn arm_writes_preserve_load_call_and_join_snapshots() {
    for source in [Snapshot::Load, Snapshot::Call, Snapshot::Join] {
        let snapshot = snapshot_across_arm_writes(source);
        for (destination, state) in [
            (1, &[0x0d, 0, 0, 0, 6, 0, 0, 0, 6, 0, 0, 0]),
            (0, &[0x0f, 0, 0, 0, 6, 0, 0, 0, 6, 0, 0, 0]),
        ] {
            let callbacks = if matches!(source, Snapshot::Call) {
                vec![
                    Call::new("receive", &[Value::I32(9)]).with_memories(&[MemoryBytes::new(
                        "state",
                        &[7, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0],
                    )]),
                ]
            } else {
                vec![]
            };
            let mut instance = snapshot.instantiate();
            assert_eq!(instance.call::<i32>((destination, 1)), Ok(17));
            assert_eq!(instance.callbacks(), &callbacks);
            assert_eq!(&instance.memory("state")[..12], state);
        }
    }
    let mut instance = snapshot_across_arm_writes(Snapshot::Join).instantiate();
    assert_eq!(instance.call::<i32>((1, 0)), Ok(17));
    assert!(instance.callbacks().is_empty());
    assert_eq!(
        &instance.memory("state")[..12],
        &[0x0d, 0, 0, 0, 0x0a, 0, 0, 0, 6, 0, 0, 0]
    );
}
