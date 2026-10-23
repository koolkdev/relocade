use crate::fixture::signature;
use relocade_compiler::{BuildError, Program, Type, Val, I1, I32, I8};
use std::{cell::RefCell, rc::Rc};

#[test]
fn a_reachable_function_must_complete_its_body() {
    let mut program = Program::new();
    let function = program.function::<fn() -> u32>(move |_, ()| Ok(()));
    program.export("run", function).unwrap();
    assert_eq!(program.compile(), Err(BuildError::MissingBody));
}

fn retained_value(fail: bool, value: u32) -> Val<I32> {
    let mut program = Program::new();
    let retained = Rc::new(RefCell::new(None));
    let capture = retained.clone();
    let function = program.function::<fn() -> u32>(move |body, ()| {
        let value = body.value::<I32>(value)?;
        *capture.borrow_mut() = Some(value.clone());
        if fail {
            Ok(())
        } else {
            body.return_(value)
        }
    });
    program.export("run", function).unwrap();
    if fail {
        assert_eq!(program.compile(), Err(BuildError::MissingBody));
    } else {
        program.compile().unwrap();
    }
    let value = retained.borrow_mut().take().unwrap();
    value
}

#[test]
fn values_from_completed_or_failed_bodies_are_rejected_by_another_builder() {
    for fail in [false, true] {
        let retained = retained_value(fail, 7);
        let mut program = Program::new();
        let function = program.function::<fn() -> u32>(move |body, ()| body.return_(retained));
        program.export("run", function).unwrap();
        assert_eq!(program.compile(), Err(BuildError::ForeignBody));
    }
}

#[test]
fn foreign_zero_is_rejected_without_poisoning_other_expressions() {
    let foreign_zero = retained_value(false, 0);
    let mut program = Program::new();
    let function = program.function::<fn(u32) -> u32>(move |body, (own,)| {
        let folded = Val::<I32>::from(0).and(&foreign_zero);
        assert_eq!(body.value(folded).err(), Some(BuildError::ForeignBody));
        assert_eq!(
            body.value(own.add(&foreign_zero).add(0)).err(),
            Some(BuildError::ForeignBody)
        );
        body.return_(own.add(11))
    });
    program.export("run", function).unwrap();
    program.compile().unwrap();
}

#[test]
fn parameter_and_return_types_must_match_the_signature() {
    for bound in [false, true] {
        let mut program = Program::new();
        let function = program.raw_function(signature(&[Type::I1], &[Type::I1]), move |body| {
            assert!(matches!(
                body.parameter::<I8>(0),
                Err(BuildError::TypeMismatch { .. })
            ));
            assert!(body.parameter::<I1>(0).is_ok());
            let value = if bound {
                body.value::<I8>(9)?
            } else {
                Val::<I8>::from(1)
            };
            body.return_(value)
        });
        program.export("run", function).unwrap();
        assert_eq!(
            program.compile(),
            Err(BuildError::TypeMismatch {
                expected: Type::I1,
                actual: Type::I8,
            })
        );
    }
}

#[test]
fn definitions_report_body_errors_during_compilation_and_cannot_be_replaced() {
    let mut program = Program::new();
    let function = program.raw_function(signature(&[], &[]), move |_| {
        Err(BuildError::UnknownParameter)
    });
    assert_eq!(
        program.define(function, move |body| body.return_(())),
        Err(BuildError::AlreadyDefined)
    );
    program.export("run", function).unwrap();
    assert_eq!(program.compile(), Err(BuildError::UnknownParameter));
}

#[test]
fn only_reachable_forward_declarations_need_definitions() {
    for called in [false, true] {
        let mut program = Program::new();
        let root = program.function::<fn()>(move |mut body, ()| {
            let forward = body.program().declare(signature(&[], &[]));
            if called {
                body.tail_call(forward, &[])
            } else {
                body.return_(())
            }
        });
        program.export("run", root).unwrap();
        let result = program.compile();
        if called {
            assert_eq!(result, Err(BuildError::MissingBody));
        } else {
            result.unwrap();
        }
    }
}

#[test]
fn body_construction_discovers_new_exports_even_without_calls() {
    let mut program = Program::new();
    let root = program.function::<fn()>(move |mut body, ()| {
        let added = body
            .program()
            .function::<fn()>(move |_, ()| Err(BuildError::UnknownParameter));
        body.program().export("added", added)?;
        body.return_(())
    });
    program.export("root", root).unwrap();
    assert_eq!(program.compile(), Err(BuildError::UnknownParameter));
}
