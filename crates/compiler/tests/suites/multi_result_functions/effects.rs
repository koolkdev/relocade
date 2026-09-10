use super::*;
use wasm86_test_support::Outcome;

fn pure_results(observe: bool, readonly: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]);
    let helper = fixture
        .program
        .function(
            signature(&[Type::I1], &[Type::I32, Type::I64]),
            |mut body| {
                let fail = body.parameter::<I1>(0)?;
                body.if_(fail, |arm| arm.trap())?;
                let first = if readonly {
                    body.load::<I32>(memory, 4)?
                } else {
                    7.into()
                };
                body.return_((first, 17_u64))
            },
        )
        .unwrap();
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let fail = body.parameter::<I1>(0)?;
        body.store::<I32>(memory, 0, 1)?;
        let (first, _unused) = body.call::<(I32, I64)>(helper, &[fail.into()])?;
        body.store::<I32>(memory, 0, 2)?;
        body.return_(if observe { first } else { 23.into() })
    })
}

#[test]
fn entirely_unused_pure_and_readonly_result_groups_can_omit_the_invocation() {
    for readonly in [false, true] {
        let module = pure_results(false, readonly);
        let code = inspect(&module, "run");
        assert_eq!(code.calls, 0);
        assert_eq!(code.drops, 0);
        for fail in [0, 1] {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>(fail).unwrap(), 23);
            assert_eq!(
                &instance.memory("state")[..10],
                &[2, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]
            );
        }
    }
}

#[test]
fn demanding_one_result_still_executes_the_callees_explicit_trap() {
    for readonly in [false, true] {
        let module = pure_results(true, readonly);
        assert_eq!(inspect(&module, "run").calls, 1);
        let mut instance = module.instantiate();
        assert_eq!(
            instance.call::<i32>(0).unwrap(),
            if readonly { 11 } else { 7 }
        );
        let mut instance = module.instantiate();
        assert!(instance.call::<i32>(1).is_err());
        // A pure or readonly call can move to its first use past disjoint stores.
        // The trap is authored by the helper, independent of memory backing size.
        assert_eq!(
            &instance.memory("state")[..10],
            &[2, 0, 0, 0, 11, 0, 0, 0, 0xa5, 0x5a]
        );
    }
}

#[test]
fn a_discarded_component_is_still_computed_by_a_retained_invocation() {
    let mut fixture = Fixture::new();
    let component = fixture
        .program
        .function(signature(&[Type::I1], &[Type::I64]), |mut body| {
            let fail = body.parameter::<I1>(0)?;
            body.if_(fail, |arm| arm.trap())?;
            body.return_(11_u64)
        })
        .unwrap();
    let helper = fixture
        .program
        .function(
            signature(&[Type::I1], &[Type::I32, Type::I64]),
            |mut body| {
                let fail = body.parameter::<I1>(0)?;
                let second = body.call::<I64>(component, &[fail.into()])?;
                body.return_((7, second))
            },
        )
        .unwrap();
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let fail = body.parameter::<I1>(0)?;
        let (first, _discarded) = body.call::<(I32, I64)>(helper, &[fail.into()])?;
        body.return_(first)
    });
    assert_eq!(inspect(&module, "run").calls, 1);
    assert_eq!(module.instantiate().call::<i32>(0).unwrap(), 7);
    assert!(module.instantiate().call::<i32>(1).is_err());
}

#[test]
fn an_unused_result_group_does_not_force_its_pure_argument_invocation() {
    let mut fixture = Fixture::new();
    let argument = fixture
        .program
        .function(signature(&[Type::I1], &[Type::I1]), |mut body| {
            let fail = body.parameter::<I1>(0)?;
            body.if_(fail, |arm| arm.trap())?;
            body.return_(true)
        })
        .unwrap();
    let helper = fixture
        .program
        .function(signature(&[Type::I1], &[Type::I32, Type::I64]), |body| {
            body.return_((7, 11_u64))
        })
        .unwrap();
    let module = fixture.function(&[], &[Type::I32], |mut body| {
        let value = body.call::<I1>(argument, &[true.into()])?;
        let _unused = body.call::<(I32, I64)>(helper, &[value.into()])?;
        body.return_(23)
    });
    assert_eq!(inspect(&module, "run").calls, 0);
    assert_eq!(module.instantiate().call::<i32>(()).unwrap(), 23);
}

#[derive(Clone, Copy, Debug)]
enum Effect {
    Write,
    WriteThenTrap,
    Import,
    Recursive,
}

const INITIAL: [u8; 14] = [7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0x5a];
const RETURNED: [Value; 3] = [Value::I32(1), Value::I64(-1), Value::I32(255)];

fn effects(behavior: Effect) -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &INITIAL);
    let shape = signature(&[Type::I32], &[Type::I1, Type::I64, Type::I8]);
    let helper = if matches!(behavior, Effect::Import) {
        fixture.callback("receive", shape.clone(), &RETURNED)
    } else {
        let helper = fixture.program.declare(shape.clone());
        let mut body = fixture.program.define(helper).unwrap();
        let input = body.parameter::<I32>(0).unwrap();
        if matches!(behavior, Effect::Recursive) {
            body.if_(input.eq(0), |arm| arm.return_((true, u64::MAX, 255)))
                .unwrap();
            body.tail_call(helper, &[input.sub(1).into()]).unwrap();
        } else {
            let calls = body.load::<I32>(memory, 4).unwrap();
            body.store(memory, 4, calls.add(1)).unwrap();
            body.store(memory, 8, input).unwrap();
            if matches!(behavior, Effect::WriteThenTrap) {
                body.trap().unwrap();
            } else {
                body.return_((true, u64::MAX, 255)).unwrap();
            }
        }
        helper
    };
    let wrapper = fixture
        .program
        .function(shape, |mut body| {
            let input = body.parameter::<I32>(0)?;
            let _unused = body.call::<(I1, I64, I8)>(helper, &[input.into()])?;
            body.return_((true, u64::MAX, 255))
        })
        .unwrap();
    fixture.program.export("wrapper", wrapper).unwrap();
    fixture.function(&[], &[], |mut body| {
        body.store::<I32>(memory, 0, 1)?;
        let _unused = body.call::<(I1, I64, I8)>(wrapper, &[7.into()])?;
        body.store::<I32>(memory, 0, 2)?;
        body.return_(())
    })
}

fn expected_memory(behavior: Effect) -> [u8; 14] {
    match behavior {
        Effect::Write => [2, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 0xa5, 0x5a],
        Effect::WriteThenTrap => [1, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 0xa5, 0x5a],
        Effect::Import | Effect::Recursive => [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0x5a],
    }
}

fn expected_callbacks(behavior: Effect) -> Vec<Call> {
    if matches!(behavior, Effect::Import) {
        vec![
            Call::new("receive", &[Value::I32(7)]).with_memories(&[MemoryBytes::new(
                "state",
                &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xa5, 0x5a],
            )]),
        ]
    } else {
        vec![]
    }
}

#[test]
fn unused_effectful_multi_result_calls_keep_one_ordered_transitive_invocation() {
    for behavior in [
        Effect::Write,
        Effect::WriteThenTrap,
        Effect::Import,
        Effect::Recursive,
    ] {
        let module = effects(behavior);
        for entry in ["run", "wrapper"] {
            let code = inspect(&module, entry);
            assert_eq!(code.calls, 1, "{behavior:?}, {entry}");
            assert_eq!(code.drops, 3, "{behavior:?}, {entry}");
        }
        let mut instance = module.instantiate();
        let result = instance.call::<()>(());
        assert_eq!(result.is_err(), matches!(behavior, Effect::WriteThenTrap));
        assert_eq!(
            &instance.memory("state")[..INITIAL.len()],
            &expected_memory(behavior)
        );
        assert_eq!(instance.callbacks(), &expected_callbacks(behavior));
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn unused_multi_result_calls_keep_host_effects_and_explicit_traps_in_v8() {
    for behavior in [Effect::Write, Effect::WriteThenTrap, Effect::Import] {
        let input = Input::call("run", &[])
            .with_memories(&[MemoryBytes::new("state", &INITIAL)])
            .with_callbacks(&[Callback::new("receive", &RETURNED)]);
        assert_eq!(
            effects(behavior).run_v8(&input),
            Observation {
                outcome: if matches!(behavior, Effect::WriteThenTrap) {
                    Outcome::Trap
                } else {
                    Outcome::Returned(vec![])
                },
                callbacks: expected_callbacks(behavior),
                memories: vec![MemoryBytes::new("state", &expected_memory(behavior))],
            }
        );
    }
}
