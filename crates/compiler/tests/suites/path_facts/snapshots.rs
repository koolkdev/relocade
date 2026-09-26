use super::*;
use crate::{fixture::signature, wasm::MemoryBytes};

fn joined_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[5, 0, 0, 0]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let flag = body.parameter::<I1>(0)?;
        let old = body.load::<I32>(memory, 0)?;
        let offset = flag.select(1, 2);
        let result = body.if_value::<I32>(
            &flag,
            |mut arm| {
                arm.store::<I32>(memory, 0, 99)?;
                arm.yield_(old.add(&offset))
            },
            |arm| arm.yield_(&old),
        )?;
        body.return_(result)
    })
}

fn call_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[5, 0, 0, 0]);
    let pair = fixture
        .program
        .function(signature(&[Type::I32], &[Type::I32, Type::I32]), |body| {
            let argument = body.parameter::<I32>(0)?;
            body.return_((argument.add(1), argument))
        })
        .unwrap();
    fixture.function(&[Type::I1], &[Type::I32, Type::I32], |mut body| {
        let flag = body.parameter::<I1>(0)?;
        let old = body.load::<I32>(memory, 0)?;
        body.if_(flag.eq(false), |arm| arm.return_((0_u32, 0_u32)))?;
        let (first, second) =
            body.call::<(I32, I32)>(pair, &[old.add(flag.select(1, 2)).argument()])?;
        body.store::<I32>(memory, 0, 99)?;
        body.return_((second, first))
    })
}

#[test]
fn a_rewritten_join_argument_preserves_its_earlier_read() {
    let module = joined_snapshot();
    for (flag, expected) in [(0, 5), (1, 6)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((flag,)), Ok(expected));
    }
}

#[test]
fn rewritten_call_arguments_preserve_snapshots_and_result_grouping() {
    let module = call_snapshot();
    for (flag, expected) in [(0, (0, 0)), (1, (6, 7))] {
        assert_eq!(
            module.instantiate().call::<(i32, i32)>((flag,)),
            Ok(expected)
        );
    }
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_rewritten_edges_preserve_snapshots() {
    for (module, expected) in [
        (joined_snapshot(), vec![Value::I32(6)]),
        (call_snapshot(), vec![Value::I32(6), Value::I32(7)]),
    ] {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[Value::I32(1)])
                    .with_memories(&[MemoryBytes::new("state", &[5, 0, 0, 0])])
            ),
            Observation::returned(&expected)
                .with_memories(&[MemoryBytes::new("state", &[99, 0, 0, 0])])
        );
    }
}
