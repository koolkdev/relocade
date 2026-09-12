use super::*;
use crate::wasm::{Callback, Input, Observation};

#[path = "guarded/continuations.rs"]
mod continuations;

const INITIAL: [u8; 12] = [7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[derive(Clone, Copy)]
enum Source {
    Load,
    ReadOnlyCall,
    Callback,
    Join,
}

fn guarded_snapshot(source: Source) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &INITIAL);
    let reader = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut body| {
            let value = body.load::<I32>(state, 0)?;
            body.return_(value)
        })
        .unwrap();
    let receive = fixture.callback("receive", signature(&[], &[Type::I32]), &[Value::I32(7)]);
    fixture.function(&[Type::I1, Type::I1, Type::I1], &[Type::I32], |mut body| {
        let first = body.parameter::<I1>(0)?;
        let second = body.parameter::<I1>(1)?;
        let choose_load = body.parameter::<I1>(2)?;
        let previous = match source {
            Source::Load => body.load::<I32>(state, 0)?,
            Source::ReadOnlyCall => body.call::<I32>(reader, &[])?,
            Source::Callback => body.call::<I32>(receive, &[])?,
            Source::Join => body.if_value::<I32>(
                choose_load,
                |mut arm| {
                    let value = arm.load::<I32>(state, 0)?;
                    arm.yield_(value)
                },
                |arm| arm.yield_(3),
            )?,
        };
        let test = previous.unsigned().ge(5);
        body.block::<()>(|mut block, _| {
            block.if_(first, |mut guarded| {
                guarded.if_(&test, |mut reached| reached.store::<I32>(state, 4, 101))
            })
        })?;
        body.store::<I32>(state, 0, 1)?;
        body.block::<()>(|mut block, _| {
            block.if_(second, |mut guarded| {
                guarded.if_(&test, |mut reached| reached.store::<I32>(state, 8, 202))
            })
        })?;
        body.return_(17)
    })
}

fn comparisons(module: &TestModule) -> Vec<usize> {
    let mut depth = 0;
    let mut depths = Vec::new();
    for event in inspect(module.bytes()) {
        match event {
            Event::If => depth += 1,
            Event::End => depth -= 1,
            Event::Compare => depths.push(depth),
            _ => {}
        }
    }
    depths
}

fn expected_memory(first: i32, second: i32) -> [u8; 12] {
    [
        1,
        0,
        0,
        0,
        if first != 0 { 101 } else { 0 },
        0,
        0,
        0,
        if second != 0 { 202 } else { 0 },
        0,
        0,
        0,
    ]
}

#[test]
fn two_guarded_tests_use_one_snapshot_across_an_intervening_write() {
    for source in [
        Source::Load,
        Source::ReadOnlyCall,
        Source::Callback,
        Source::Join,
    ] {
        let module = guarded_snapshot(source);
        assert_eq!(comparisons(&module), [1, 1]);
        let production = if matches!(source, Source::ReadOnlyCall | Source::Callback) {
            Event::Call
        } else {
            Event::Load
        };
        assert_eq!(
            inspect(module.bytes())
                .iter()
                .filter(|&&event| event == production)
                .count(),
            1
        );
        for (first, second) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>((first, second, 1)), Ok(17));
            assert_eq!(
                &instance.memory("state")[..12],
                expected_memory(first, second)
            );
            let expected_calls = if matches!(source, Source::Callback) {
                vec![Call::new("receive", &[]).with_memories(&[MemoryBytes::new("state", &INITIAL)])]
            } else {
                vec![]
            };
            assert_eq!(instance.callbacks(), expected_calls);
        }
    }
    let mut instance = guarded_snapshot(Source::Join).instantiate();
    assert_eq!(instance.call::<i32>((1, 1, 0)), Ok(17));
    assert_eq!(&instance.memory("state")[..12], expected_memory(0, 0));
}

fn repeated_tests(count: usize, guarded: bool, returned: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 12]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let enabled = body.parameter::<I1>(0)?;
        let test = body.parameter::<I32>(1)?.unsigned().ge(5);
        for index in 0..count {
            body.block::<()>(|mut block, _| {
                if guarded {
                    block.if_(&enabled, |mut arm| {
                        arm.if_(&test, |mut reached| {
                            reached.store::<I32>(state, index as u32 * 4, 1)
                        })
                    })
                } else {
                    block.if_(&test, |mut reached| {
                        reached.store::<I32>(state, index as u32 * 4, 1)
                    })
                }
            })?;
        }
        if returned {
            body.return_(test.unsigned().extend::<I32>())
        } else {
            body.return_(17)
        }
    })
}

#[test]
fn three_control_regions_keep_a_shared_test() {
    for (count, guarded, returned) in [(3, true, false), (2, true, true)] {
        let module = repeated_tests(count, guarded, returned);
        assert_eq!(comparisons(&module), [0]);
        for input in [4, 7] {
            let mut instance = module.instantiate();
            assert_eq!(
                instance.call::<i32>((1, input)),
                Ok(if returned { i32::from(input >= 5) } else { 17 })
            );
            let mut expected = [0; 12];
            for index in 0..count {
                expected[index * 4] = u8::from(input >= 5);
            }
            assert_eq!(&instance.memory("state")[..12], expected);
        }
    }
}

#[test]
fn two_transparent_blocks_each_compute_their_test() {
    let module = repeated_tests(2, false, false);
    assert_eq!(comparisons(&module), [0, 0]);
    for (input, expected) in [(4, [0; 12]), (7, [1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0])] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((1, input)), Ok(17));
        assert_eq!(&instance.memory("state")[..12], expected);
    }
}

#[test]
fn each_of_two_regions_shares_its_test_with_its_arithmetic() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 12]);
    let module = fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let enabled = body.parameter::<I1>(0)?;
        let test = body.parameter::<I32>(1)?.unsigned().ge(5);
        let number = test.unsigned().extend::<I32>().add(10);
        for offset in [0, 4] {
            body.block::<()>(|mut block, _| {
                block.if_(&enabled, |mut guarded| {
                    guarded.if_(&test, |mut reached| reached.store::<I32>(state, 8, 1))?;
                    guarded.store(state, offset, &number)
                })
            })?;
        }
        body.return_(17)
    });
    assert_eq!(comparisons(&module), [1, 1]);
    for (input, value, marked) in [(4, 10, 0), (7, 11, 1)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((1, input)), Ok(17));
        assert_eq!(
            &instance.memory("state")[..12],
            [value, 0, 0, 0, value, 0, 0, 0, marked, 0, 0, 0]
        );
    }
}

#[test]
fn single_truth_uses_in_separate_guards_need_no_saved_boolean() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    let module = fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let first = body.parameter::<I1>(0)?;
            let second = body.parameter::<I1>(1)?;
            let test = body.parameter::<I32>(2)?.ne(0);
            for (enabled, offset) in [(&first, 0), (&second, 4)] {
                body.block::<()>(|mut block, _| {
                    block.if_(enabled, |mut guarded| {
                        guarded.if_(&test, |mut reached| reached.store::<I32>(state, offset, 1))
                    })
                })?;
            }
            body.return_(17)
        },
    );
    let events = inspect(module.bytes());
    assert!(!events.contains(&Event::ZeroTest));
    assert!(!events.contains(&Event::LocalWrite));
    for (first, second, input, expected) in [
        (0, 0, 7, [0; 8]),
        (1, 0, 7, [1, 0, 0, 0, 0, 0, 0, 0]),
        (0, 1, 7, [0, 0, 0, 0, 1, 0, 0, 0]),
        (1, 1, 7, [1, 0, 0, 0, 1, 0, 0, 0]),
        (1, 1, 0, [0; 8]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((first, second, input)), Ok(17));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

#[test]
fn exclusive_arms_share_repeated_inputs_without_repeating_snapshot_reads() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0, 0, 0, 0]);
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let previous = body.load::<I32>(state, 0)?;
        let changed = previous.xor(16);
        body.if_else(
            condition,
            |mut arm| arm.store(state, 4, changed.add(&changed)),
            |mut arm| arm.store(state, 4, &changed),
        )?;
        body.return_(17)
    });
    let events = inspect(module.bytes());
    assert_eq!(
        events.iter().filter(|&&event| event == Event::Load).count(),
        1
    );
    assert_eq!(
        events.iter().filter(|&&event| event == Event::Xor).count(),
        2
    );
    assert_eq!(xor_counts_on_paths(&events, &mut 0), [1, 1]);
    for (condition, value) in [(0, 23), (1, 46)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(condition), Ok(17));
        assert_eq!(&instance.memory("state")[..8], [7, 0, 0, 0, value, 0, 0, 0]);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn guarded_predicates_preserve_load_call_and_join_snapshots_in_v8() {
    for source in [
        Source::Load,
        Source::ReadOnlyCall,
        Source::Callback,
        Source::Join,
    ] {
        let module = guarded_snapshot(source);
        for (first, second) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let mut input = Input::call(
                "run",
                &[Value::I32(first), Value::I32(second), Value::I32(1)],
            )
            .with_memories(&[MemoryBytes::new("state", &INITIAL)]);
            let mut expected = Observation::returned(&[Value::I32(17)])
                .with_memories(&[MemoryBytes::new("state", &expected_memory(first, second))]);
            if matches!(source, Source::Callback) {
                input = input.with_callbacks(&[Callback::new("receive", &[Value::I32(7)])]);
                expected = expected
                    .with_callbacks(&[Call::new("receive", &[])
                        .with_memories(&[MemoryBytes::new("state", &INITIAL)])]);
            }
            assert_eq!(module.run_v8(&input), expected);
        }
    }
}
