use wasm86_compiler::{BuildError, Program, Signature, Type, I1, I32, I64, I8};

#[test]
fn dropping_a_body_leaves_its_function_unfinished() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I32,
    });
    let body = program.define(function).unwrap();
    body.constant::<I32>(7);
    drop(body);
    assert!(matches!(program.compile(), Err(BuildError::MissingBody)));
}

#[test]
fn values_from_a_completed_body_are_rejected_by_another_builder() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![],
        result: Type::I32,
    };
    let first = program.declare(signature.clone());
    let second = program.declare(signature);
    let body = program.define(first).unwrap();
    let retained = body.constant::<I32>(7);
    body.return_(&retained).unwrap();

    let body = program.define(second).unwrap();
    assert!(body.return_(&retained).is_err());
    let body = program.define(second).unwrap();
    let result = body.constant::<I32>(9);
    body.return_(&result).unwrap();
    program.compile().unwrap();
}

#[test]
fn restarting_a_body_rejects_its_old_values() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I64,
    });
    let body = program.define(function).unwrap();
    let retained = body.constant::<I64>(7);
    drop(body);

    let body = program.define(function).unwrap();
    let result = body.constant::<I64>(9).add(&retained);
    assert!(body.return_(&result).is_err());
    assert!(matches!(program.compile(), Err(BuildError::MissingBody)));
}

#[test]
fn foreign_zero_is_rejected_without_poisoning_other_expressions() {
    let mut foreign_program = Program::new();
    let foreign_function = foreign_program.declare(Signature {
        parameters: vec![],
        result: Type::I32,
    });
    let foreign_body = foreign_program.define(foreign_function).unwrap();
    let foreign_zero = foreign_body.constant::<I32>(0);

    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        result: Type::I32,
    });
    let body = program.define(function).unwrap();
    let own = body.parameter::<I32>(0).unwrap();
    let invalid = own.add(&foreign_zero).add(0);
    assert!(matches!(
        body.return_(&invalid),
        Err(BuildError::ForeignBody)
    ));

    let body = program.define(function).unwrap();
    let own = body.parameter::<I32>(0).unwrap();
    let invalid = own.add(&foreign_zero);
    let valid = invalid.c::<I32>(11).add(&own);
    body.return_(&valid).unwrap();
    program.compile().unwrap();
}

#[test]
fn parameter_and_return_types_must_match_the_signature() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I1],
        result: Type::I1,
    });
    let body = program.define(function).unwrap();
    assert!(matches!(
        body.parameter::<I8>(0),
        Err(BuildError::TypeMismatch { .. })
    ));
    let other_type = body.constant::<I8>(9);
    assert!(matches!(
        body.return_(&other_type),
        Err(BuildError::TypeMismatch { .. })
    ));
    let body = program.define(function).unwrap();
    let result = body.parameter::<I1>(0).unwrap();
    body.return_(&result).unwrap();
    program.compile().unwrap();
}
