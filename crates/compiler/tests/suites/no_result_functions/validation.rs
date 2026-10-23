use crate::fixture::signature;
use relocade_compiler::{BuildError, FunctionImport, Program, Type, I8};
use wasmparser::{Parser, Payload, Validator};

#[test]
fn ordinary_and_tail_calls_require_the_declared_result_presence() {
    for expects_result in [false, true] {
        let mut program = Program::new();
        let void = program.import_function(FunctionImport {
            module: "test".into(),
            name: "void".into(),
            signature: signature(&[], &[]),
        });
        let value = program.import_function(FunctionImport {
            module: "test".into(),
            name: "value".into(),
            signature: signature(&[], &[Type::I8]),
        });
        let results = if expects_result {
            vec![Type::I8]
        } else {
            vec![]
        };
        let run = program.raw_function(signature(&[], &results), move |mut body| {
            assert_eq!(
                body.call::<I8>(void, &[]).err(),
                Some(BuildError::ResultCount {
                    expected: 1,
                    actual: 0
                })
            );
            assert_eq!(
                body.call::<()>(value, &[]),
                Err(BuildError::ResultCount {
                    expected: 0,
                    actual: 1
                })
            );
            body.tail_call(if expects_result { void } else { value }, &[])
        });
        program.export("run", run).unwrap();
        assert_eq!(
            program.compile(),
            Err(BuildError::ResultCount {
                expected: usize::from(expects_result),
                actual: usize::from(!expects_result),
            })
        );
    }
}

#[test]
fn returns_require_the_declared_result_presence() {
    for expects_result in [false, true] {
        let mut program = Program::new();
        let results = if expects_result {
            vec![Type::I8]
        } else {
            vec![]
        };
        let run = program.raw_function(signature(&[], &results), move |body| {
            if expects_result {
                body.return_(())
            } else {
                body.return_(7)
            }
        });
        program.export("run", run).unwrap();
        assert_eq!(
            program.compile(),
            Err(BuildError::ResultCount {
                expected: usize::from(expects_result),
                actual: usize::from(!expects_result),
            })
        );
    }
}

#[test]
fn invalid_void_calls_validate_all_arguments_without_retaining_imports() {
    let mut program = Program::new();
    let target = program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: signature(&[Type::I8], &[]),
    });
    let foreign = crate::fixture::capture_from_body(|body| body.value::<I8>(7).unwrap());
    let run = program.declare(signature(&[], &[]));
    program
        .define(run, move |mut body| {
            assert_eq!(
                body.call::<()>(target, &[]),
                Err(BuildError::ArgumentCount {
                    expected: 1,
                    actual: 0
                })
            );
            assert_eq!(
                body.call::<()>(target, &[7_u64.into()]),
                Err(BuildError::TypeMismatch {
                    expected: Type::I8,
                    actual: Type::I64
                })
            );
            assert_eq!(
                body.call::<()>(target, &[foreign.into()]),
                Err(BuildError::ForeignBody)
            );
            body.return_(())
        })
        .unwrap();
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|part| !matches!(part.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn an_ignored_invalid_void_return_cannot_become_a_fallthrough_branch() {
    let mut program = Program::new();
    let run = program.declare(signature(&[], &[Type::I8]));
    program
        .define(run, move |mut body| {
            assert_eq!(
                body.if_(true, |arm| {
                    assert!(arm.return_(()).is_err());
                    Ok(())
                }),
                Err(BuildError::ResultCount {
                    expected: 1,
                    actual: 0
                })
            );
            body.return_(7)
        })
        .unwrap();
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
}

#[test]
fn invalid_arguments_to_a_new_helper_fail_compilation() {
    let mut program = Program::new();
    let failure = program.function::<fn()>(move |mut body, ()| {
        let helper = body
            .program()
            .function::<fn()>(move |body, ()| body.return_(()));
        body.call::<()>(helper, &[7.into()])?;
        body.return_(())
    });
    program.export("run", failure).unwrap();
    assert_eq!(
        program.compile().err(),
        Some(BuildError::ArgumentCount {
            expected: 0,
            actual: 1
        })
    );
}
