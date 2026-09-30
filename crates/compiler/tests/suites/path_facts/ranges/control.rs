//! Interval knowledge belongs to its guarded path and immutable input snapshot.

use super::*;

fn sibling_and_join() -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let result = body.if_value::<I32>(
            input.unsigned().lt(10),
            |arm| arm.yield_(input.unsigned().lt(20).select::<I32>(1, 99)),
            |arm| arm.yield_(input.unsigned().lt(20).select::<I32>(2, 3)),
        )?;
        body.return_(result.add(input.unsigned().lt(20).select::<I32>(10, 0)))
    })
}

fn bypass() -> TestModule {
    Fixture::new().function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let skip = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?;
        body.block::<()>(|mut block, exit| {
            block.branch_if(skip, &exit, ())?;
            block.if_(input.unsigned().ge(20), |arm| arm.return_(7))?;
            block.yield_(())
        })?;
        body.return_(input.unsigned().lt(30).select::<I32>(1, 2))
    })
}

fn loop_ranges() -> TestModule {
    Fixture::new().function(&[], &[Type::I32], |mut body| {
        let sum =
            body.loop_::<(I32, I32), I32>((0, 0), |mut iteration, labels, (index, sum)| {
                iteration.branch_if(index.unsigned().ge(4), &labels.exit, &sum)?;
                let term = iteration.if_value::<I32>(
                    index.unsigned().lt(2),
                    |arm| arm.yield_(index.unsigned().lt(3).select::<I32>(10, 99)),
                    |arm| arm.yield_(index.unsigned().lt(3).select::<I32>(20, 30)),
                )?;
                iteration.branch(&labels.again, (index.add(1), sum.add(term)))
            })?;
        body.return_(sum)
    })
}

#[test]
fn unsigned_ranges_do_not_escape_siblings_joins_or_bypassed_guards() {
    let module = sibling_and_join();
    let mut instance = module.instantiate();
    for (input, expected) in [(0, 11), (9, 11), (10, 12), (19, 12), (20, 3), (-1, 3)] {
        assert_eq!(instance.call::<i32>((input,)), Ok(expected));
    }
    let module = bypass();
    let mut instance = module.instantiate();
    for (skip, input, expected) in [(0, 19, 1), (0, 20, 7), (1, 19, 1), (1, 20, 1), (1, 30, 2)] {
        assert_eq!(instance.call::<i32>((skip, input)), Ok(expected));
    }
}

#[test]
fn loop_carried_values_receive_fresh_ranges_on_each_iteration() {
    assert_eq!(loop_ranges().instantiate().call::<i32>(()), Ok(70));
}

#[test]
fn a_new_memory_snapshot_does_not_inherit_an_earlier_range() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &9_u32.to_le_bytes());
    let module = fixture.function(&[], &[Type::I32], |mut body| {
        let before = body.load::<I32>(memory, 0)?;
        body.if_(before.unsigned().ge(20), |arm| arm.return_(99))?;
        body.store::<I32>(memory, 0, 30)?;
        let after = body.load::<I32>(memory, 0)?;
        body.return_(
            before
                .unsigned()
                .lt(20)
                .unsigned()
                .extend::<I32>()
                .add(after.unsigned().lt(20).select::<I32>(10, 0)),
        )
    });
    assert_eq!(module.instantiate().call::<i32>(()), Ok(1));
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_range_facts_preserve_control_flow_boundaries() {
    let module = sibling_and_join();
    for (input, expected) in [(9, 11), (19, 12), (20, 3)] {
        check_result(&module, &[Value::I32(input)], &[Value::I32(expected)], true);
    }
    check_result(&loop_ranges(), &[], &[Value::I32(70)], true);
    check_result(
        &bypass(),
        &[Value::I32(1), Value::I32(30)],
        &[Value::I32(2)],
        true,
    );
}
