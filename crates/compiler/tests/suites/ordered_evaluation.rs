use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{BuildError, Type, I1, I16, I32, I64, I8};
use wasm86_test_support::Outcome;

const INITIAL: [u8; 16] = [
    0x44, 0x33, 0x22, 0x11, 5, 0, 0, 0, 6, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c,
];

fn unused_read(offset: u32) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &INITIAL);
    fixture.function(&[], Some(Type::I32), |mut body| {
        let read = body.load::<I32>(state, offset)?;
        body.store::<I32>(state, 4, 1)?;
        body.evaluate(read)?;
        body.store::<I32>(state, 8, 2)?;
        body.return_(17)
    })
}

fn reused_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &INITIAL);
    fixture.function(&[Type::I32], Some(Type::I64), |mut body| {
        let address = body.parameter::<I32>(0)?;
        let read = body.load_at::<I32>(state, address, 0)?;
        let snapshot = read.unsigned().extend::<I64>();
        body.evaluate(&snapshot)?;
        body.store::<I16>(state, 1, 0x9988)?;
        body.store::<I8>(state, 3, 0x55)?;
        body.evaluate(&snapshot)?;
        body.store(state, 8, &snapshot)?;
        body.return_(snapshot.add(&snapshot))
    })
}

fn unused_join() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &INITIAL);
    fixture.function(&[Type::I1], Some(Type::I32), |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let _unused = body.if_value::<I32>(
            condition,
            |mut arm| {
                let read = arm.load::<I32>(state, 65536)?;
                arm.store::<I32>(state, 4, 1)?;
                arm.evaluate(&read)?;
                arm.yield_(read)
            },
            |mut arm| {
                let read = arm.load::<I32>(state, 65536)?;
                arm.store::<I32>(state, 4, 2)?;
                arm.yield_(read)
            },
        )?;
        body.store::<I32>(state, 8, 3)?;
        body.return_(17)
    })
}

fn required_read_call(wrapped: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &INITIAL);
    let reader = fixture
        .program
        .function(signature(&[Type::I32], Some(Type::I32)), |mut body| {
            let address = body.parameter::<I32>(0)?;
            let read = body.load_at::<I32>(state, address, 0)?;
            body.evaluate(&read)?;
            body.return_(read)
        })
        .unwrap();
    let target = if wrapped {
        fixture
            .program
            .function(signature(&[Type::I32], None), |mut body| {
                let address = body.parameter::<I32>(0)?;
                let _unused = body.call::<I32>(reader, &[address.argument()])?;
                body.return_void()
            })
            .unwrap()
    } else {
        reader
    };
    fixture.function(&[Type::I32], Some(Type::I32), |mut body| {
        let address = body.parameter::<I32>(0)?;
        body.store::<I32>(state, 4, 1)?;
        if wrapped {
            body.call_void(target, &[address.argument()])?;
        } else {
            let _unused = body.call::<I32>(target, &[address.argument()])?;
        }
        body.store::<I32>(state, 8, 2)?;
        body.return_(17)
    })
}

#[test]
fn unused_evaluated_reads_trap_after_prior_stores_and_before_later_stores() {
    let mut instance = unused_read(65536).instantiate();
    assert!(instance.call::<i32>(()).is_err());
    assert_eq!(
        &instance.memory("state")[..16],
        &[0x44, 0x33, 0x22, 0x11, 1, 0, 0, 0, 6, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
    );

    let mut instance = unused_read(0).instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(17));
    assert_eq!(
        &instance.memory("state")[..16],
        &[0x44, 0x33, 0x22, 0x11, 1, 0, 0, 0, 2, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
    );
}

#[test]
fn evaluated_expressions_reuse_read_snapshots_across_overlapping_stores() {
    let mut instance = reused_snapshot().instantiate();
    assert_eq!(instance.call::<i64>(0), Ok(0x2244_6688));
    assert_eq!(
        &instance.memory("state")[..16],
        &[0x44, 0x88, 0x99, 0x55, 5, 0, 0, 0, 0x44, 0x33, 0x22, 0x11, 0, 0, 0, 0]
    );
}

#[test]
fn evaluated_branch_reads_remain_required_when_the_join_result_is_unused() {
    let module = unused_join();
    let mut instance = module.instantiate();
    assert!(instance.call::<i32>(1).is_err());
    assert_eq!(
        &instance.memory("state")[..16],
        &[0x44, 0x33, 0x22, 0x11, 1, 0, 0, 0, 6, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
    );

    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(0), Ok(17));
    assert_eq!(
        &instance.memory("state")[..16],
        &[0x44, 0x33, 0x22, 0x11, 2, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
    );
}

#[test]
fn evaluation_rejects_child_reads_outside_their_branch() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &INITIAL);
    let module = fixture.function(&[Type::I1], Some(Type::I32), |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let mut escaped = None;
        body.if_(condition, |mut arm| {
            escaped = Some(arm.load::<I32>(state, 65536)?);
            Ok(())
        })?;
        assert_eq!(body.evaluate(escaped.unwrap()), Err(BuildError::OutOfScope));
        body.store::<I32>(state, 8, 2)?;
        body.return_(17)
    });
    for condition in [0, 1] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(condition), Ok(17));
        assert_eq!(
            &instance.memory("state")[..16],
            &[0x44, 0x33, 0x22, 0x11, 5, 0, 0, 0, 2, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
        );
    }
}

#[test]
fn required_reads_keep_unused_value_calls_and_void_wrappers() {
    for wrapped in [false, true] {
        let module = required_read_call(wrapped);
        let mut instance = module.instantiate();
        assert!(instance.call::<i32>(65536).is_err(), "wrapped={wrapped}");
        assert_eq!(
            &instance.memory("state")[..16],
            &[0x44, 0x33, 0x22, 0x11, 1, 0, 0, 0, 6, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
        );

        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(0), Ok(17), "wrapped={wrapped}");
        assert_eq!(
            &instance.memory("state")[..16],
            &[0x44, 0x33, 0x22, 0x11, 1, 0, 0, 0, 2, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c]
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 smoke tests"]
fn v8_ordered_evaluation_preserves_traps_scope_and_snapshots() {
    let helper = required_read_call(true);
    let input = Input::call("run", &[Value::I32(65536)])
        .with_memories(&[MemoryBytes::new("state", &INITIAL)]);
    assert_eq!(
        helper.run_v8(&input),
        Observation {
            outcome: Outcome::Trap,
            callbacks: vec![],
            memories: vec![MemoryBytes::new(
                "state",
                &[0x44, 0x33, 0x22, 0x11, 1, 0, 0, 0, 6, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c],
            )],
        }
    );

    let branch = unused_join();
    let input =
        Input::call("run", &[Value::I32(0)]).with_memories(&[MemoryBytes::new("state", &INITIAL)]);
    assert_eq!(
        branch.run_v8(&input),
        Observation::returned(Value::I32(17)).with_memories(&[MemoryBytes::new(
            "state",
            &[0x44, 0x33, 0x22, 0x11, 2, 0, 0, 0, 3, 0, 0, 0, 0xa5, 0x5a, 0xc3, 0x3c],
        )])
    );

    let snapshot = reused_snapshot();
    assert_eq!(
        snapshot.run_v8(&input),
        Observation::returned(Value::I64(0x2244_6688)).with_memories(&[MemoryBytes::new(
            "state",
            &[0x44, 0x88, 0x99, 0x55, 5, 0, 0, 0, 0x44, 0x33, 0x22, 0x11, 0, 0, 0, 0],
        )])
    );
}
