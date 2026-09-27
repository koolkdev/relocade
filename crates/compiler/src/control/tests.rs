use crate::{BuildError, FunctionImport, MemoryImport, Program, Signature, Type, I32, I8};
use wasmparser::{Parser, Payload};

mod exits;
mod labels;

#[test]
fn a_failed_else_branch_discards_both_arms_without_closing_the_parent() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let signature = Signature {
        parameters: vec![],
        results: vec![Type::I32],
    };
    let target = program.import_function(FunctionImport {
        module: "test".into(),
        name: "target".into(),
        signature: signature.clone(),
    });
    let function = program.declare(signature);
    program
        .define(function, |mut body| {
            assert_eq!(
                body.if_else(
                    true,
                    |mut branch| {
                        branch.store::<I32>(memory, 0, 9)?;
                        branch.call::<I32>(target, &[])?;
                        branch.if_(true, |inner| inner.tail_call(target, &[]))
                    },
                    |branch| {
                        branch.parameter::<I32>(0)?;
                        Ok(())
                    },
                ),
                Err(BuildError::UnknownParameter)
            );
            body.return_(7)
        })
        .unwrap();
    let bytes = program.compile().unwrap();
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn an_ignored_return_error_discards_the_branch_and_keeps_the_parent_open() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    program
        .define(function, |mut body| {
            assert_eq!(
                body.if_(true, |branch| {
                    assert!(branch.return_(true).is_err());
                    Ok(())
                }),
                Err(BuildError::TypeMismatch {
                    expected: Type::I32,
                    actual: Type::I1
                })
            );
            body.return_(7)
        })
        .unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn completing_a_child_keeps_parent_values_open_until_the_function_completes() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    let mut retained = None;
    program
        .define(function, |mut body| {
            let value = body.value::<I32>(7).unwrap();
            body.if_(false, |branch| branch.return_(&value)).unwrap();
            let result = value.add(1);
            retained = Some((result.clone(), body.arena.clone()));
            body.return_(&result)
        })
        .unwrap();
    let (result, arena) = retained.unwrap();
    assert_eq!(
        result.checked_expression(&arena, 0),
        Err(BuildError::BodyClosed)
    );
    assert!(program.compile().is_ok());
}

#[test]
fn an_ignored_yield_error_discards_both_arms_without_retaining_their_imports() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let signature = Signature {
        parameters: vec![],
        results: vec![Type::I32],
    };
    let target = program.import_function(FunctionImport {
        module: "test".into(),
        name: "target".into(),
        signature: signature.clone(),
    });
    let function = program.declare(signature);
    program
        .define(function, |mut body| {
            let result = body.if_value::<I32>(
                true,
                |mut arm| {
                    arm.store::<I32>(memory, 0, 9)?;
                    arm.yield_(7)
                },
                |mut arm| {
                    arm.call::<I32>(target, &[])?;
                    let byte = arm.value::<I8>(1)?;
                    assert!(arm.yield_(byte).is_err());
                    Ok(())
                },
            );
            assert_eq!(
                result.err(),
                Some(BuildError::TypeMismatch {
                    expected: Type::I32,
                    actual: Type::I8
                })
            );
            body.return_(7)
        })
        .unwrap();
    let bytes = program.compile().unwrap();
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn yielding_a_nested_join_exposes_only_the_new_parent_result() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    let mut retained = None;
    program
        .define(function, |mut body| {
            let mut escaped = None;
            let selected = body
                .if_value::<I32>(
                    true,
                    |mut arm| {
                        let nested = arm.if_value::<I32>(
                            false,
                            |inner| inner.yield_(1),
                            |inner| inner.yield_(2),
                        )?;
                        escaped = Some(nested.clone());
                        arm.yield_(nested)
                    },
                    |arm| arm.yield_(3),
                )
                .unwrap();
            let escaped = escaped.unwrap();
            for value in [escaped.add(1), escaped.and(0).add(1)] {
                assert_eq!(body.value(value).err(), Some(BuildError::OutOfScope));
            }
            let result = selected.add(1);
            retained = Some((result.clone(), body.arena.clone()));
            body.return_(&result)
        })
        .unwrap();
    let (result, arena) = retained.unwrap();
    assert_eq!(
        result.checked_expression(&arena, 0),
        Err(BuildError::BodyClosed)
    );
    assert!(program.compile().is_ok());
}
