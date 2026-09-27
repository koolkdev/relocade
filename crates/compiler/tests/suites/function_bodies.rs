use wasm86_compiler::{BuildError, Program, Signature, Type, Val, I1, I32, I64, I8};

#[test]
fn a_function_cannot_fall_through() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    assert_eq!(
        program.define(function, |_| Ok(())),
        Err(BuildError::MissingBody)
    );
    assert!(matches!(program.compile(), Err(BuildError::MissingBody)));
}

#[test]
fn values_from_a_completed_body_are_rejected_by_another_builder() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![],
        results: vec![Type::I32],
    };
    let first = program.declare(signature.clone());
    let second = program.declare(signature);
    let mut retained = None;
    program
        .define(first, |body| {
            let value = body.value::<I32>(7)?;
            retained = Some(value.clone());
            body.return_(value)
        })
        .unwrap();
    assert_eq!(
        program.define(second, |body| body.return_(retained.unwrap())),
        Err(BuildError::ForeignBody)
    );
    program.define(second, |body| body.return_(9)).unwrap();
    program.compile().unwrap();
}

#[test]
fn restarting_a_body_rejects_its_old_values() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I64],
    });
    let mut retained = None;
    assert_eq!(
        program.define(function, |body| {
            retained = Some(body.value::<I64>(7)?);
            Ok(())
        }),
        Err(BuildError::MissingBody)
    );
    assert_eq!(
        program.define(function, |body| {
            let result = body.value::<I64>(9)?.add(retained.as_ref().unwrap());
            body.return_(result)
        }),
        Err(BuildError::ForeignBody)
    );
    assert!(matches!(program.compile(), Err(BuildError::MissingBody)));
}

#[test]
fn foreign_zero_is_rejected_without_poisoning_other_expressions() {
    let mut foreign_program = Program::new();
    let mut foreign_zero = None;
    foreign_program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I32],
            },
            |foreign_body| {
                foreign_zero = Some(foreign_body.value::<I32>(0)?);
                foreign_body.return_(0)
            },
        )
        .unwrap();
    let foreign_zero = foreign_zero.unwrap();
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
    });
    assert_eq!(
        program.define(function, |body| {
            let own = body.parameter::<I32>(0)?;
            let folded = Val::<I32>::from(0).and(&foreign_zero);
            assert_eq!(body.value(folded).err(), Some(BuildError::ForeignBody));
            body.return_(own.add(&foreign_zero).add(0))
        }),
        Err(BuildError::ForeignBody)
    );
    program
        .define(function, |body| {
            let own = body.parameter::<I32>(0)?;
            let _invalid = own.add(&foreign_zero);
            body.return_(own.add(11))
        })
        .unwrap();
    program.compile().unwrap();
}

#[test]
fn parameter_and_return_types_must_match_the_signature() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I1],
        results: vec![Type::I1],
    });
    assert_eq!(
        program.define(function, |body| {
            assert!(matches!(
                body.parameter::<I8>(0),
                Err(BuildError::TypeMismatch { .. })
            ));
            let other_type = body.value::<I8>(9)?;
            body.return_(other_type)
        }),
        Err(BuildError::TypeMismatch {
            expected: Type::I1,
            actual: Type::I8
        })
    );
    assert_eq!(
        program.define(function, |body| body.return_(Val::<I8>::from(1))),
        Err(BuildError::TypeMismatch {
            expected: Type::I1,
            actual: Type::I8
        })
    );
    program
        .define(function, |body| {
            let result = body.parameter::<I1>(0)?;
            body.return_(result)
        })
        .unwrap();
    program.compile().unwrap();
}
