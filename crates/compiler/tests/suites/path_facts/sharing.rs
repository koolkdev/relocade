use super::*;
use crate::wasm::MemoryBytes;

#[derive(Clone, Copy)]
enum Flow {
    Fallthrough,
    BlockExit,
    Return,
    PublishAndReturn,
    StoreThenReturn,
}

fn conditional_chain(depth: usize, flow: Flow) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 4]);
    fixture.function(&[Type::I32, Type::I32], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let count = body.parameter::<I32>(1)?.and(31);
        let active = count.ne(0);
        let mut result = input;
        for _ in 0..depth {
            result = active.select(result.unsigned().shr(&count).or(0x8000_0000_u32), &result);
        }
        if matches!(flow, Flow::StoreThenReturn) {
            body.store(memory, 0, &result)?;
        }
        match flow {
            Flow::Fallthrough => {
                body.if_(&active, |mut arm| arm.store(memory, 0, &result))?;
            }
            Flow::BlockExit => {
                body.block::<()>(|mut block, done| {
                    block.if_(&active, |mut arm| {
                        arm.store(memory, 0, &result)?;
                        arm.branch(&done, ())
                    })?;
                    // Skipping the first arm proves this repeated condition false.
                    block.if_(&active, |arm| arm.return_(17))?;
                    block.yield_(())
                })?;
            }
            Flow::Return | Flow::PublishAndReturn | Flow::StoreThenReturn => {
                body.if_(&active, |mut arm| {
                    if matches!(flow, Flow::PublishAndReturn) {
                        arm.store(memory, 0, &result)?;
                    }
                    arm.return_(&result)
                })?;
                body.if_(&active, |arm| arm.return_(17))?;
                if matches!(flow, Flow::PublishAndReturn) {
                    body.store(memory, 0, &result)?;
                }
            }
        }
        body.return_(result)
    })
}

fn chain_result(mut value: u32, count: u32, depth: usize) -> u32 {
    let count = count & 31;
    if count != 0 {
        for _ in 0..depth {
            value = (value >> count) | 0x8000_0000;
        }
    }
    value
}

#[test]
fn a_branch_and_continuation_reuse_the_same_calculation_chain() {
    for (depth, flow) in [1, 4, 16]
        .into_iter()
        .flat_map(|depth| [Flow::Fallthrough, Flow::BlockExit].map(|flow| (depth, flow)))
    {
        let module = conditional_chain(depth, flow);
        assert_eq!(count(&module, |op| matches!(op, Operator::I32ShrU)), depth);
        assert_eq!(count(&module, |op| matches!(op, Operator::I32Or)), depth);
        assert_eq!(count(&module, |op| matches!(op, Operator::If { .. })), 1);
        for shift in [0, 1, 3, 31, 32, 255] {
            let expected = chain_result(0x1234_5678, shift, depth);
            let mut instance = module.instantiate();
            assert_eq!(
                instance.call::<i32>((0x1234_5678, shift as i32)),
                Ok(expected as i32)
            );
            let stored = if shift & 31 != 0 { expected } else { 0 };
            assert_eq!(&instance.memory("state")[..4], &stored.to_le_bytes());
        }
    }
}

#[test]
fn returning_paths_preserve_values_and_publication() {
    for flow in [Flow::Return, Flow::PublishAndReturn] {
        let module = conditional_chain(4, flow);
        for shift in [0, 1, 3, 31] {
            let expected = chain_result(0x1234_5678, shift as u32, 4);
            let mut instance = module.instantiate();
            assert_eq!(
                instance.call::<i32>((0x1234_5678, shift)),
                Ok(expected as i32)
            );
            let stored = if matches!(flow, Flow::PublishAndReturn) {
                expected
            } else {
                0
            };
            assert_eq!(&instance.memory("state")[..4], &stored.to_le_bytes());
        }
    }
}

#[test]
fn a_calculation_used_before_a_branch_remains_shared_with_its_return() {
    let module = conditional_chain(4, Flow::StoreThenReturn);
    assert_eq!(count(&module, |op| matches!(op, Operator::I32ShrU)), 4);
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Or)), 4);
    for shift in [0, 1, 3, 31] {
        let expected = chain_result(0x1234_5678, shift as u32, 4);
        let mut instance = module.instantiate();
        assert_eq!(
            instance.call::<i32>((0x1234_5678, shift)),
            Ok(expected as i32)
        );
        assert_eq!(&instance.memory("state")[..4], &expected.to_le_bytes());
    }
}

#[test]
fn facts_before_shared_uses_still_simplify_the_common_calculation() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 4]);
    let module = fixture.function(
        &[Type::I1, Type::I1, Type::I32],
        &[Type::I32],
        |mut body| {
            let stop = body.parameter::<I1>(0)?;
            let publish = body.parameter::<I1>(1)?;
            let input = body.parameter::<I32>(2)?;
            let number = stop.select(&input, input.add(1));
            let square = number.mul(&number);
            body.if_(&stop, |arm| arm.return_(7))?;
            body.if_(publish, |mut arm| arm.store(memory, 0, &square))?;
            body.return_(square)
        },
    );
    assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 0);
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Mul)), 1);
    for (stop, publish, expected, stored) in [(1, 1, 7, 0_u32), (0, 0, 36, 0), (0, 1, 36, 36)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((stop, publish, 5)), Ok(expected));
        assert_eq!(&instance.memory("state")[..4], &stored.to_le_bytes());
    }
}

#[test]
fn a_returning_group_can_specialize_despite_enclosing_reuse() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 8]);
    let module = fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let flag = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?;
        let number = flag.select(input, 7);
        let square = number.mul(&number);
        body.if_else(
            flag,
            |mut arm| arm.store(memory, 0, &square),
            |mut arm| {
                arm.store(memory, 4, &square)?;
                arm.return_(&square)
            },
        )?;
        body.return_(square)
    });
    // The returning arm knows the number is seven, even though the other arm
    // and its continuation need a shared, nonconstant square.
    assert!(count(&module, |op| matches!(op, Operator::I32Const { value: 49 })) > 0);
    for (flag, expected, memory) in [
        (0, 49, [0, 0, 0, 0, 49, 0, 0, 0]),
        (1, 25, [25, 0, 0, 0, 0, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((flag, 5)), Ok(expected));
        assert_eq!(&instance.memory("state")[..8], memory);
    }
}

#[test]
fn facts_about_a_shared_predicate_survive_its_replacement_by_an_alias() {
    let module = Fixture::new().function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let first = body.parameter::<I1>(0)?;
        let second = body.parameter::<I1>(1)?;
        let either = first.select(true, second);
        body.if_(&first, |arm| arm.return_(7))?;
        body.if_(&either, |arm| arm.return_(11))?;
        body.if_(&either, |arm| arm.return_(13))?;
        body.return_(17)
    });
    assert_eq!(count(&module, |op| matches!(op, Operator::If { .. })), 2);
    for (first, second, expected) in [(0, 0, 17), (0, 1, 11), (1, 0, 7), (1, 1, 7)] {
        assert_eq!(
            module.instantiate().call::<i32>((first, second)),
            Ok(expected)
        );
    }
}

fn nested_exit() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 8]);
    fixture.function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let skip = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?;
        let number = skip.select(&input, input.add(1));
        let square = number.mul(&number);
        body.block::<()>(|mut outer, done| {
            outer.block::<()>(|mut inner, _| {
                inner.if_(&skip, |mut arm| {
                    arm.store(memory, 0, &square)?;
                    arm.branch(&done, ())
                })?;
                inner.yield_(())
            })?;
            outer.store::<I32>(memory, 4, 99)?;
            outer.yield_(())
        })?;
        body.return_(square)
    })
}

#[test]
fn a_nested_outward_exit_shares_with_the_destination_continuation() {
    let module = nested_exit();
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Mul)), 1);
    for (skip, expected, memory) in [
        (0, 36, [0, 0, 0, 0, 99, 0, 0, 0]),
        (1, 25, [25, 0, 0, 0, 0, 0, 0, 0]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((skip, 5)), Ok(expected));
        assert_eq!(&instance.memory("state")[..8], memory);
    }
}

fn alternating_loop() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[42, 0, 0, 0, 0, 0, 0, 0]);
    fixture.function(&[Type::I32, Type::I1], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let initial = body.parameter::<I1>(1)?;
        let result = body.loop_::<(I32, I1), I32>(
            (count, initial),
            |mut iteration, labels, (left, flag)| {
                let number = flag.select(&left, left.add(1));
                let square = number.mul(&number);
                iteration.if_(&flag, |mut arm| arm.store(memory, 0, &square))?;
                iteration.store(memory, 4, &square)?;
                iteration.branch_if(left.eq(0), &labels.exit, &square)?;
                iteration.branch(&labels.again, (left.sub(1), flag.eq(false)))
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn shared_calculations_use_the_current_iteration_inputs() {
    let module = alternating_loop();
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Mul)), 1);
    for (count, initial, expected, stored) in [
        (0, 0, 1, 42_u32),
        (0, 1, 0, 0),
        (3, 0, 0, 0),
        (3, 1, 1, 1),
        (4, 0, 1, 1),
        (4, 1, 0, 0),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((count, initial)), Ok(expected));
        assert_eq!(&instance.memory("state")[..4], &stored.to_le_bytes());
        assert_eq!(&instance.memory("state")[4..8], &expected.to_le_bytes());
    }
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_shared_and_returning_paths_preserve_results() {
    for flow in [
        Flow::Fallthrough,
        Flow::BlockExit,
        Flow::Return,
        Flow::PublishAndReturn,
        Flow::StoreThenReturn,
    ] {
        let module = conditional_chain(16, flow);
        for shift in [0, 3, 31] {
            let expected = chain_result(0x1234_5678, shift, 16);
            let stored = match flow {
                Flow::StoreThenReturn | Flow::PublishAndReturn => expected,
                Flow::Fallthrough | Flow::BlockExit if shift != 0 => expected,
                _ => 0,
            };
            assert_eq!(
                module.run_v8(
                    &Input::call("run", &[Value::I32(0x1234_5678), Value::I32(shift as i32)])
                        .with_memories(&[MemoryBytes::new("state", &[0; 4])])
                ),
                Observation::returned(&[Value::I32(expected as i32)])
                    .with_memories(&[MemoryBytes::new("state", &stored.to_le_bytes())])
            );
        }
    }
    for (module, input, expected, memory) in [
        (nested_exit(), [1, 5], 25, [25, 0, 0, 0, 0, 0, 0, 0]),
        (nested_exit(), [0, 5], 36, [0, 0, 0, 0, 99, 0, 0, 0]),
        (alternating_loop(), [3, 0], 0, [0; 8]),
        (alternating_loop(), [3, 1], 1, [1, 0, 0, 0, 1, 0, 0, 0]),
    ] {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &input.map(Value::I32))
                    .with_memories(&[MemoryBytes::new("state", &[0; 8])])
            ),
            Observation::returned(&[Value::I32(expected)])
                .with_memories(&[MemoryBytes::new("state", &memory)])
        );
    }
}
