use super::*;

fn guard_and_continuation() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5; 8]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let enabled = body.parameter::<I1>(0)?;
        let masked = body.parameter::<I32>(1)?.and(0xff);
        body.if_(enabled, |mut arm| arm.store(state, 0, &masked))?;
        body.store(state, 4, &masked)?;
        body.return_(masked)
    })
}

#[test]
fn a_guard_and_its_continuation_use_the_same_mask() {
    let module = guard_and_continuation();
    for (enabled, input, result, expected) in [
        (0, 0x1234, 52, [0xa5, 0xa5, 0xa5, 0xa5, 52, 0, 0, 0]),
        (1, 0x1234, 52, [52, 0, 0, 0, 52, 0, 0, 0]),
        (0, -1, 255, [0xa5, 0xa5, 0xa5, 0xa5, 255, 0, 0, 0]),
        (1, -1, 255, [255, 0, 0, 0, 255, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((enabled, input)), Ok(result));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

fn block_exit_and_continuation() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5; 8]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let skip = body.parameter::<I1>(0)?;
        let sum = body.parameter::<I32>(1)?.add(7);
        body.block::<()>(|mut block, success| {
            block.if_(skip, |arm| arm.branch(&success, ()))?;
            block.store(state, 0, &sum)?;
            block.return_(&sum)
        })?;
        body.store(state, 4, &sum)?;
        body.return_(sum)
    })
}

#[test]
fn a_block_suffix_and_its_continuation_use_the_same_sum() {
    let module = block_exit_and_continuation();
    for (skip, input, result, expected) in [
        (0, 9, 16, [16, 0, 0, 0, 0xa5, 0xa5, 0xa5, 0xa5]),
        (1, 9, 16, [0xa5, 0xa5, 0xa5, 0xa5, 16, 0, 0, 0]),
        (0, -7, 0, [0, 0, 0, 0, 0xa5, 0xa5, 0xa5, 0xa5]),
        (1, -7, 0, [0xa5, 0xa5, 0xa5, 0xa5, 0, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((skip, input)), Ok(result));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

fn block_with_alternative_continuations() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 12]);
    fixture.function(
        &[Type::I1, Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let continue_to_arms = body.parameter::<I1>(0)?;
            let first_arm = body.parameter::<I1>(1)?;
            let use_in_block = body.parameter::<I1>(2)?;
            let sum = body.parameter::<I32>(3)?.add(5);
            let first_arm = body.block::<I1>(|mut block, continuation| {
                block.if_(continue_to_arms, |arm| arm.branch(&continuation, first_arm))?;
                block.if_(use_in_block, |mut arm| {
                    arm.store(state, 0, &sum)?;
                    arm.return_(&sum)
                })?;
                block.return_(17)
            })?;
            body.if_else(
                first_arm,
                |mut arm| {
                    arm.store(state, 4, &sum)?;
                    arm.return_(&sum)
                },
                |mut arm| {
                    arm.store(state, 8, &sum)?;
                    arm.return_(&sum)
                },
            )?;
            body.trap()
        },
    )
}

fn check_alternative_continuations(v8: bool) {
    use crate::wasm::{Input, Observation};

    let module = block_with_alternative_continuations();
    for (arguments, result, memory) in [
        ([0, 0, 0, 9], 17, [0; 12]),
        ([0, 0, 1, 9], 14, [14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        ([1, 1, 0, 9], 14, [0, 0, 0, 0, 14, 0, 0, 0, 0, 0, 0, 0]),
        ([1, 0, 1, 9], 14, [0, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0, 0]),
    ] {
        if v8 {
            assert_eq!(
                module.run_v8(
                    &Input::call("run", &arguments.map(Value::I32))
                        .with_memories(&[MemoryBytes::new("state", &[0; 12])])
                ),
                Observation::returned(&[Value::I32(result)])
                    .with_memories(&[MemoryBytes::new("state", &memory)])
            );
        } else {
            let mut instance = module.instantiate();
            let [continue_to_arms, first_arm, use_in_block, input] = arguments;
            assert_eq!(
                instance.call::<i32>((continue_to_arms, first_arm, use_in_block, input)),
                Ok(result)
            );
            assert_eq!(&instance.memory("state")[..12], memory);
        }
    }
}

#[test]
fn alternative_continuations_preserve_results_and_effects() {
    check_alternative_continuations(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn alternative_continuations_preserve_results_and_effects_in_v8() {
    check_alternative_continuations(true);
}

#[test]
fn conditional_writes_and_a_return_use_the_same_calculation() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    let module = fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let first = body.parameter::<I1>(0)?;
            let second = body.parameter::<I1>(1)?;
            let shared = body.parameter::<I32>(2)?.xor(0x55);
            body.if_(first, |mut arm| arm.store(state, 0, &shared))?;
            body.if_(second, |mut arm| arm.store(state, 4, &shared))?;
            body.return_(shared)
        },
    );
    for (first, second, expected) in [
        (0, 0, [0; 8]),
        (1, 0, [82, 0, 0, 0, 0, 0, 0, 0]),
        (0, 1, [0, 0, 0, 0, 82, 0, 0, 0]),
        (1, 1, [82, 0, 0, 0, 82, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((first, second, 7)), Ok(82));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

#[test]
fn a_derived_result_is_available_in_a_guard_and_its_continuation() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    let module = fixture.function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let enabled = body.parameter::<I1>(0)?;
            let left = body.parameter::<I32>(1)?;
            let right = body.parameter::<I32>(2)?;
            let derived = left.mul(right).xor(1);
            body.if_(enabled, |mut arm| arm.store(state, 0, &derived))?;
            body.store(state, 4, &derived)?;
            body.return_(derived)
        },
    );
    for (enabled, expected) in [
        (0, [0, 0, 0, 0, 34, 0, 0, 0]),
        (1, [34, 0, 0, 0, 34, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((enabled, 5, 7)), Ok(34));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}

fn repeated_input_diamonds(depth: usize) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0; 8]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let enabled = body.parameter::<I1>(0)?;
        let mut shared = body.parameter::<I32>(1)?;
        for marker in 1..=depth {
            let fork = shared.xor(marker as u32);
            shared = fork.add(&fork);
        }
        body.if_(enabled, |mut arm| arm.store(state, 0, &shared))?;
        body.store(state, 4, &shared)?;
        body.return_(shared)
    })
}

#[test]
fn repeated_input_diamonds_have_linear_code_growth() {
    for depth in [1, 4, 16] {
        let events = inspect(repeated_input_diamonds(depth).bytes());
        // Sharing may duplicate a calculation across paths, but must not expand
        // repeated dependencies into an exponential expression tree.
        for operation in [Event::Xor, Event::Add] {
            let count = events.iter().filter(|&&event| event == operation).count();
            assert!(
                count <= 2 * depth,
                "{depth} levels produced {count} operations"
            );
        }
    }
    let module = repeated_input_diamonds(3);
    for (enabled, expected) in [
        (0, [0, 0, 0, 0, 62, 0, 0, 0]),
        (1, [62, 0, 0, 0, 62, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        // Input 7 gives 12, then 28, then 62 at the three merges.
        assert_eq!(instance.call::<i32>((enabled, 7)), Ok(62));
        assert_eq!(&instance.memory("state")[..8], expected);
    }
}
