use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, TestModule, Value};

#[path = "no_result_functions/effects.rs"]
mod effects;
#[path = "no_result_functions/validation.rs"]
mod validation;

use wasm86_compiler::{Type, I1, I32, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn shared_writer() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let writer = fixture
        .program
        .function(signature(&[Type::I32], None), |mut body| {
            let value = body.parameter::<I32>(0)?;
            body.store(state, 0, value)?;
            body.return_void()
        })
        .unwrap();
    fixture.function(&[Type::I32], None, |mut body| {
        let input = body.parameter::<I32>(0)?;
        body.call_void(writer, &[input.add(1).into()])?;
        body.return_void()
    })
}

fn imported_tail() -> TestModule {
    let mut fixture = Fixture::new();
    let receive = fixture.callback("receive", signature(&[Type::I8, Type::I8], None), None);
    let relay = fixture
        .program
        .function(signature(&[Type::I8], None), |body| {
            let value = body.parameter::<I8>(0)?.add(1);
            body.tail_call(receive, &[(&value).into(), value.into()])
        })
        .unwrap();
    fixture.function(&[Type::I8], None, |mut body| {
        let value = body.parameter::<I8>(0)?.add(1);
        body.call_void(relay, &[value.into()])?;
        body.return_void()
    })
}

fn mutating_call() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let mutator = fixture
        .program
        .function(signature(&[], None), |mut body| {
            body.store::<I32>(state, 0, 9)?;
            body.return_void()
        })
        .unwrap();
    let wrapper = fixture
        .program
        .function(signature(&[], None), |body| body.tail_call(mutator, &[]))
        .unwrap();
    fixture.function(&[], Some(Type::I32), |mut body| {
        let before = body.load::<I32>(state, 0)?;
        body.call_void(wrapper, &[])?;
        let after = body.load::<I32>(state, 0)?;
        body.return_(before.add(after))
    })
}

fn returning_value_arm(use_switch: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[1, 0, 0, 0]);
    fixture.function(&[Type::I1], None, |mut body| {
        let stop = body.parameter::<I1>(0)?;
        let value = if use_switch {
            body.switch_value::<I32, _>(stop, &[1], |arm, key| match key {
                Some(1) => arm.return_void(),
                _ => arm.yield_(7),
            })?
        } else {
            body.if_value::<I32>(stop, |arm| arm.return_void(), |arm| arm.yield_(7))?
        };
        body.store(state, 0, value)?;
        body.return_void()
    })
}

fn conditional_calls() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let writer = fixture
        .program
        .function(signature(&[Type::I32, Type::I32], None), |mut body| {
            let first = body.parameter::<I32>(0)?;
            let second = body.parameter::<I32>(1)?;
            body.store(state, 0, first)?;
            body.store(state, 4, second)?;
            body.return_void()
        })
        .unwrap();
    fixture.function(&[Type::I1], None, |mut body| {
        let write = body.parameter::<I1>(0)?;
        let before = body.load::<I32>(state, 0)?;
        let shared = before.add(1);
        body.if_(write, |mut arm| {
            arm.call_void(writer, &[(&shared).into(), (&shared).into()])
        })?;
        body.store(state, 8, shared)?;
        body.return_void()
    })
}

fn recursive_tail() -> TestModule {
    let mut fixture = Fixture::new();
    let run = fixture.program.declare(signature(&[Type::I32], None));
    let mut body = fixture.program.define(run).unwrap();
    let remaining = body.parameter::<I32>(0).unwrap();
    body.if_(remaining.eq(0), |arm| arm.return_void()).unwrap();
    body.tail_call(run, &[remaining.sub(1).into()]).unwrap();
    fixture.finish(run)
}

#[test]
fn no_result_functions_have_empty_wasm_results_and_no_discarded_value() {
    let module = shared_writer();
    let bytes = module.bytes();
    Validator::new().validate_all(bytes).unwrap();
    let mut calls = 0;
    let mut returns = 0;
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::TypeSection(types) => {
                for ty in types.into_iter_err_on_gc_types() {
                    assert!(ty.unwrap().results().is_empty());
                }
            }
            Payload::CodeSectionEntry(body) => {
                bodies += 1;
                assert_eq!(body.get_locals_reader().unwrap().get_count(), 0);
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::Call { .. } => calls += 1,
                        Operator::Return => returns += 1,
                        Operator::Drop => panic!("a no-result call has no value to discard"),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!((bodies, calls, returns), (2, 1, 2));
}

#[test]
fn no_result_calls_validate_with_imports_tails_effects_and_value_arms() {
    for module in [
        imported_tail(),
        mutating_call(),
        returning_value_arm(false),
        returning_value_arm(true),
        conditional_calls(),
        recursive_tail(),
    ] {
        Validator::new().validate_all(module.bytes()).unwrap();
    }
}

#[test]
fn void_writers_share_values_with_their_caller() {
    let mut instance = shared_writer().instantiate();
    instance.call::<()>(41).unwrap();
    assert_eq!(&instance.memory("state")[..4], &[0x2a, 0, 0, 0]);
}

#[test]
fn void_imported_tail_calls_preserve_arguments_and_absent_results() {
    let mut instance = imported_tail().instantiate();
    instance.call::<()>(254).unwrap();
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(0), Value::I32(0)])]
    );
}

#[test]
fn void_callee_writes_preserve_prior_snapshots() {
    let mut instance = mutating_call().instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 16);
    assert_eq!(&instance.memory("state")[..4], &[9, 0, 0, 0]);
}

#[test]
fn void_returns_from_value_arms_skip_continuations() {
    for use_switch in [false, true] {
        let module = returning_value_arm(use_switch);
        let mut instance = module.instantiate();
        instance.call::<()>(1).unwrap();
        assert_eq!(&instance.memory("state")[..4], &[1, 0, 0, 0]);

        let mut instance = module.instantiate();
        instance.call::<()>(0).unwrap();
        assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
    }
}

#[test]
fn void_conditional_calls_execute_only_reached_effects() {
    let module = conditional_calls();
    let mut instance = module.instantiate();
    instance.call::<()>(1).unwrap();
    assert_eq!(
        &instance.memory("state")[..12],
        &[8, 0, 0, 0, 8, 0, 0, 0, 8, 0, 0, 0]
    );

    let mut instance = module.instantiate();
    instance.call::<()>(0).unwrap();
    assert_eq!(
        &instance.memory("state")[..12],
        &[7, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0]
    );
}

#[test]
fn void_tail_recursion_does_not_grow_the_call_stack() {
    recursive_tail().instantiate().call::<()>(300000).unwrap();
}
