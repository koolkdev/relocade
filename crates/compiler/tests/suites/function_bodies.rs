use wasm86_compiler::{BuildError, Program, Signature, Type, I1, I32, I64, I8};

#[test]
fn dropping_a_body_leaves_its_function_unfinished() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        result: Some(Type::I32),
    });
    let body = program.define(function).unwrap();
    drop(body);
    assert!(matches!(program.compile(), Err(BuildError::MissingBody)));
}

#[test]
fn values_from_a_completed_body_are_rejected_by_another_builder() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![],
        result: Some(Type::I32),
    };
    let first = program.declare(signature.clone());
    let second = program.declare(signature);
    let body = program.define(first).unwrap();
    let retained = body.value::<I32>(7).unwrap();
    body.return_(&retained).unwrap();

    let body = program.define(second).unwrap();
    assert!(body.return_(&retained).is_err());
    let body = program.define(second).unwrap();
    body.return_(9).unwrap();
    program.compile().unwrap();
}

#[test]
fn restarting_a_body_rejects_its_old_values() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        result: Some(Type::I64),
    });
    let body = program.define(function).unwrap();
    let retained = body.value::<I64>(7).unwrap();
    drop(body);

    let body = program.define(function).unwrap();
    let result = body.value::<I64>(9).unwrap().add(&retained);
    assert!(body.return_(&result).is_err());
    assert!(matches!(program.compile(), Err(BuildError::MissingBody)));
}

#[test]
fn foreign_zero_is_rejected_without_poisoning_other_expressions() {
    let mut foreign_program = Program::new();
    let foreign_function = foreign_program.declare(Signature {
        parameters: vec![],
        result: Some(Type::I32),
    });
    let foreign_body = foreign_program.define(foreign_function).unwrap();
    let foreign_zero = foreign_body.value::<I32>(0).unwrap();

    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        result: Some(Type::I32),
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
    let _invalid = own.add(&foreign_zero);
    let valid = own.add(11);
    body.return_(&valid).unwrap();
    program.compile().unwrap();
}

#[test]
fn parameter_and_return_types_must_match_the_signature() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I1],
        result: Some(Type::I1),
    });
    let body = program.define(function).unwrap();
    assert!(matches!(
        body.parameter::<I8>(0),
        Err(BuildError::TypeMismatch { .. })
    ));
    let other_type = body.value::<I8>(9).unwrap();
    assert!(matches!(
        body.return_(&other_type),
        Err(BuildError::TypeMismatch { .. })
    ));
    let body = program.define(function).unwrap();
    let result = body.parameter::<I1>(0).unwrap();
    body.return_(&result).unwrap();
    program.compile().unwrap();
}
