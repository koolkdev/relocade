use crate::{BuildError, MemoryImport, Program, Signature, Type, I1, I32};
use wasmparser::{Operator, Parser, Payload, Validator};

#[test]
fn unwinding_a_definition_closes_its_values_and_allows_retry() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    let mut retained = None;
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = program.define(function, |body| {
            retained = Some((body.value::<I32>(7)?, body.arena.clone()));
            panic!("construction interrupted");
        });
    }));
    assert!(panic.is_err());
    let (value, arena) = retained.unwrap();
    assert_eq!(
        value.checked_expression(&arena, 0),
        Err(BuildError::BodyClosed)
    );
    program.define(function, |body| body.return_(9)).unwrap();
    Validator::new()
        .validate_all(&program.compile().unwrap())
        .unwrap();
}

#[test]
fn an_ignored_return_error_leaves_the_function_undefined_and_allows_retry() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    assert_eq!(
        program.define(function, |body| {
            assert!(body.return_(true).is_err());
            Ok(())
        }),
        Err(BuildError::TypeMismatch {
            expected: Type::I32,
            actual: Type::I1,
        })
    );
    program.define(function, |body| body.return_(7)).unwrap();
    Validator::new()
        .validate_all(&program.compile().unwrap())
        .unwrap();
}

#[test]
fn completed_functions_can_be_called_and_exported_with_logical_signatures() {
    let mut program = Program::new();
    let is_zero = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I1],
            },
            |body| {
                let value = body.parameter::<I32>(0)?;
                body.return_(value.eq(0))
            },
        )
        .unwrap();
    let run = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I1],
            },
            |mut body| {
                let input = body.parameter::<I32>(0)?;
                let result = body.call::<I1>(is_zero, &[input.into()])?;
                body.return_(result)
            },
        )
        .unwrap();
    program.export("is_zero", is_zero).unwrap();
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let calls = Parser::new(0)
        .parse_all(&bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(
                body.get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .filter(|operator| matches!(operator, Ok(Operator::Call { .. })))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(calls, 1);
}

#[test]
fn a_callback_error_discards_even_a_completed_body_and_its_import_use() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "host".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let error = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I32],
            },
            |mut body| {
                let value = body.load::<I32>(memory, 0)?;
                body.return_(value)?;
                Err(BuildError::BodyClosed)
            },
        )
        .unwrap_err();
    assert_eq!(error, BuildError::BodyClosed);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I32],
            },
            |body| body.return_(7),
        )
        .unwrap();
    assert_eq!(function.0, 0);
    program.export("run", function).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn a_function_body_requires_a_terminal() {
    let mut program = Program::new();
    let error = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I1],
            },
            |_| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error, BuildError::MissingBody);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I1],
            },
            |body| body.return_(true),
        )
        .unwrap();
    assert_eq!(function.0, 0);
    program.export("run", function).unwrap();
    Validator::new()
        .validate_all(&program.compile().unwrap())
        .unwrap();
}

#[test]
fn an_open_body_builds_a_helper_without_reopening_itself() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I1],
    };
    let outer = program.declare(signature.clone());
    program
        .define(outer, |mut body| {
            let error = body
                .program()
                .function(signature.clone(), |helper| {
                    helper.trap()?;
                    Err(BuildError::BodyClosed)
                })
                .unwrap_err();
            assert_eq!(error, BuildError::BodyClosed);
            assert_eq!(
                body.program().define(outer, |body| body.trap()).err(),
                Some(BuildError::AlreadyDefined)
            );
            let input = body.parameter::<I32>(0).unwrap();
            let helper = body
                .program()
                .function(signature, |helper| {
                    let value = helper.parameter::<I32>(0)?;
                    helper.return_(value.eq(0))
                })
                .unwrap();
            body.if_(input.eq(-1), |branch| branch.trap()).unwrap();
            let result = body.call::<I1>(helper, &[input.into()]).unwrap();
            body.return_(result)
        })
        .unwrap();
    program.export("run", outer).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut traps = 0;
    let mut calls = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                match op.unwrap() {
                    Operator::Unreachable => traps += 1,
                    Operator::Call { .. } => calls += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!((traps, calls), (1, 1));
}

#[test]
fn a_failed_function_restores_forward_declarations_and_discards_appended_resources() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![],
        results: vec![Type::I32],
    };
    let prior = program
        .function(signature.clone(), |body| body.return_(11))
        .unwrap();
    program.export("prior", prior).unwrap();
    let forward = program.declare(signature.clone());
    let error = program
        .function(signature.clone(), |mut body| {
            let memory = body.program().import_memory(MemoryImport {
                module: "discarded".into(),
                name: "memory".into(),
                minimum: 1,
                maximum: None,
                shared: false,
            });
            let helper = body.program().function(signature.clone(), |mut helper| {
                let value = helper.load::<I32>(memory, 0)?;
                helper.return_(value)
            })?;
            body.program().export("discarded", helper)?;
            body.program().define(forward, |mut forward_body| {
                let value = forward_body.call::<I32>(helper, &[])?;
                forward_body.return_(value)
            })?;
            body.trap()?;
            Err(BuildError::BodyClosed)
        })
        .unwrap_err();
    assert_eq!(error, BuildError::BodyClosed);
    assert_eq!(program.functions.len(), 2);
    assert!(program.memories.is_empty());
    assert_eq!(program.exports.len(), 1);
    // Its discarded body referred to the removed helper and must not survive.
    program.define(forward, |body| body.return_(23)).unwrap();
    let replacement = program.function(signature, |body| body.return_(7)).unwrap();
    assert_eq!(replacement.0, 2);
    program.export("discarded", replacement).unwrap();
    Validator::new()
        .validate_all(&program.compile().unwrap())
        .unwrap();
}
