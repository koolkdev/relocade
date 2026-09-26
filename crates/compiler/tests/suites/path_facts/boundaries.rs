use super::*;
use wasm86_compiler::I8;

#[test]
fn a_new_load_does_not_inherit_facts_about_an_earlier_snapshot() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0xfe]);
    let module = fixture.function(&[], &[Type::I8, Type::I8], |mut body| {
        let before = body.load::<I8>(memory, 0)?;
        body.if_(before.truncate::<I1>(), |arm| arm.return_((0_u32, 0_u32)))?;
        body.store::<I8>(memory, 0, 1)?;
        let after = body.load::<I8>(memory, 0)?;
        body.return_((before, after))
    });
    assert_eq!(module.instantiate().call::<(i32, i32)>(()), Ok((0xfe, 1)));
}

#[test]
fn an_early_block_exit_can_bypass_a_later_check() {
    let module = Fixture::new().function(&[Type::I1, Type::I1], &[Type::I1], |mut body| {
        let skip = body.parameter::<I1>(0)?;
        let flag = body.parameter::<I1>(1)?;
        body.block::<()>(|mut block, exit| {
            block.branch_if(skip, &exit, ())?;
            block.if_(&flag, |arm| arm.return_(false))?;
            block.yield_(())
        })?;
        body.return_(flag)
    });
    for (skip, flag, expected) in [(0, 0, 0), (0, 1, 0), (1, 0, 0), (1, 1, 1)] {
        assert_eq!(module.instantiate().call::<i32>((skip, flag)), Ok(expected));
    }
}

#[test]
fn an_early_yield_does_not_export_facts_from_the_arms_suffix() {
    let module = Fixture::new().function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let skip = body.parameter::<I1>(0)?;
        let flag = body.parameter::<I1>(1)?;
        let result = body.if_value::<I32>(
            true,
            |mut arm| {
                arm.yield_if(skip, 10)?;
                arm.if_(&flag, |fault| fault.return_(20))?;
                arm.yield_(30)
            },
            |arm| arm.yield_(40),
        )?;
        body.return_(result.add(flag.unsigned().extend::<I32>()))
    });
    for (skip, flag, expected) in [(0, 0, 30), (0, 1, 20), (1, 0, 10), (1, 1, 11)] {
        assert_eq!(module.instantiate().call::<i32>((skip, flag)), Ok(expected));
    }
}

#[test]
fn loop_inputs_are_reconsidered_on_each_backedge() {
    let module = Fixture::new().function(&[Type::I1], &[Type::I32], |mut body| {
        let initial = body.parameter::<I1>(0)?;
        body.loop_::<I1, ()>(initial, |mut iteration, labels, flag| {
            iteration.if_(&flag, |arm| arm.return_(27))?;
            iteration.branch(&labels.again, flag.eq(false))
        })?;
        body.return_(99)
    });
    for input in 0..2 {
        assert_eq!(module.instantiate().call::<i32>((input,)), Ok(27));
    }
}

#[test]
fn a_yielding_arm_does_not_prove_its_condition_at_the_join() {
    let module = Fixture::new().function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let number = body.if_value::<I32>(&condition, |arm| arm.yield_(7), |arm| arm.yield_(11))?;
        body.return_(number.add(condition.select(13, 17)))
    });
    assert_eq!(module.instantiate().call::<i32>((1,)), Ok(20));
    assert_eq!(module.instantiate().call::<i32>((0,)), Ok(28));
}
