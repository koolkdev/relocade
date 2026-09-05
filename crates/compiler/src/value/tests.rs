use super::Val;
use crate::{BuildError, Program, Signature, Type, I32, I64};

fn assert_closed(value: &Val<I32>) {
    assert_eq!(value.add(0).expression, Err(BuildError::BodyClosed));
    assert_eq!(value.c::<I64>(0).expression, Err(BuildError::BodyClosed));
}

#[test]
fn returning_from_a_body_closes_retained_values() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I32,
    });
    let body = program.define(function).unwrap();
    let value = body.constant::<I32>(7);
    body.return_(&value).unwrap();
    assert_closed(&value);
    assert!(program.compile().is_ok());
}

#[test]
fn dropping_a_body_closes_retained_values() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I32,
    });
    let body = program.define(function).unwrap();
    let value = body.constant::<I32>(7);
    drop(body);
    assert_closed(&value);
}

#[test]
fn a_failed_return_closes_retained_values() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I64,
    });
    let body = program.define(function).unwrap();
    let value = body.constant::<I32>(7);
    assert_eq!(
        body.return_(&value),
        Err(BuildError::TypeMismatch {
            expected: Type::I64,
            actual: Type::I32,
        })
    );
    assert_closed(&value);
}
