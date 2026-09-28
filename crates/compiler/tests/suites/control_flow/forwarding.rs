//! Result forwarding must preserve the destinations of earlier exits.
use crate::{fixture::Fixture, wasm::TestModule};
use wasm86_compiler::{Type, I1, I32, I8};

#[test]
fn identical_value_arms_still_define_their_join_result() {
    let module = Fixture::new().function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let result = body.if_value::<I32>(condition, |arm| arm.yield_(7), |arm| arm.yield_(7))?;
        body.return_(result)
    });
    for condition in [0, 1] {
        assert_eq!(module.instantiate().call::<i32>((condition,)), Ok(7));
    }
}

#[test]
fn shared_outgoing_bits_retain_each_destinations_logical_type() {
    let module = Fixture::new().function(&[Type::I1, Type::I8], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let byte = body.parameter::<I8>(1)?;
        let result = body.block::<I8>(|mut outer, exit| {
            let widened = outer.block::<I32>(|mut inner, _| {
                inner.branch_if(condition, &exit, &byte)?;
                inner.yield_(byte.unsigned().extend::<I32>())
            })?;
            outer.yield_(widened.add(1).truncate::<I8>())
        })?;
        body.return_(result.unsigned().extend::<I32>())
    });
    for (condition, byte, expected) in [(0, 7, 8), (1, 7, 7), (0, 255, 0), (1, 255, 255)] {
        assert_eq!(
            module.instantiate().call::<i32>((condition, byte)),
            Ok(expected)
        );
    }
}

fn seed_with_outward_exit(use_loop: bool) -> TestModule {
    Fixture::new().function(&[Type::I1], &[Type::I32], |mut body| {
        let leave = body.parameter::<I1>(0)?;
        let result = body.block::<I32>(|mut block, exit| {
            let seed =
                block.if_value::<I32>(leave, |arm| arm.branch(&exit, 41), |arm| arm.yield_(7))?;
            let result = if use_loop {
                block.loop_::<I32, I32>(seed, |iteration, labels, input| {
                    iteration.branch(&labels.exit, input.add(5))
                })?
            } else {
                block.switch_value::<I32, _>(seed, &[7], |arm, key| {
                    arm.yield_(if key.is_some() { 12 } else { 19 })
                })?
            };
            block.yield_(result)
        })?;
        body.return_(result)
    })
}

#[test]
fn loop_and_switch_entry_keep_the_destinations_of_seed_calculations() {
    for use_loop in [false, true] {
        let module = seed_with_outward_exit(use_loop);
        for (leave, result) in [(0, 12), (1, 41)] {
            assert_eq!(module.instantiate().call::<i32>((leave,)), Ok(result));
        }
    }
}

#[test]
fn two_outward_arms_skip_the_entire_remaining_block() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        body.block::<()>(|mut block, exit| {
            block.if_else(
                condition,
                |arm| arm.branch(&exit, ()),
                |arm| arm.branch(&exit, ()),
            )?;
            block.store::<I32>(state, 0, 99)
        })?;
        let result = body.load::<I32>(state, 0)?;
        body.return_(result)
    });
    for condition in [0, 1] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((condition,)), Ok(7));
        assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
    }
}

#[test]
fn a_snapshot_used_as_a_loop_seed_remains_available_after_the_loop() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let module = fixture.function(&[], &[Type::I32, Type::I32], |mut body| {
        let original = body.load::<I32>(state, 0)?;
        let result = body.loop_::<I32, I32>(&original, |mut iteration, labels, input| {
            iteration.store::<I32>(state, 0, 99)?;
            iteration.branch(&labels.exit, input.add(5))
        })?;
        body.return_((original, result))
    });
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<(i32, i32)>(()), Ok((7, 12)));
    assert_eq!(&instance.memory("state")[..4], &[99, 0, 0, 0]);
}
