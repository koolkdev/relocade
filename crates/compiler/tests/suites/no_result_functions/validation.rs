use crate::fixture::signature;
use wasm86_compiler::{BuildError, FunctionImport, Program, Type, I8};
use wasmparser::{Parser, Payload, Validator};

#[test]
fn ordinary_and_tail_calls_require_the_declared_result_presence() {
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
    let run = program.declare(signature(&[], &[]));
    let mut body = program.define(run).unwrap();
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
    assert_eq!(
        body.tail_call(value, &[]),
        Err(BuildError::ResultCount {
            expected: 0,
            actual: 1
        })
    );
    let typed = program.declare(signature(&[], &[Type::I8]));
    assert_eq!(
        program.define(typed).unwrap().tail_call(void, &[]),
        Err(BuildError::ResultCount {
            expected: 1,
            actual: 0
        })
    );
    program.define(typed).unwrap().return_(7).unwrap();
    program.define(run).unwrap().return_(()).unwrap();
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|part| !matches!(part.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn invalid_returns_leave_the_function_undefined_and_can_be_retried() {
    let mut program = Program::new();
    let void = program.declare(signature(&[], &[]));
    assert_eq!(
        program.define(void).unwrap().return_(7),
        Err(BuildError::ResultCount {
            expected: 0,
            actual: 1
        })
    );
    program.define(void).unwrap().return_(()).unwrap();
    let value = program.declare(signature(&[], &[Type::I8]));
    assert_eq!(
        program.define(value).unwrap().return_(()),
        Err(BuildError::ResultCount {
            expected: 1,
            actual: 0
        })
    );
    program.define(value).unwrap().return_(7).unwrap();
    program.export("run", void).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
}

#[test]
fn invalid_void_calls_validate_all_arguments_without_retaining_imports() {
    let mut program = Program::new();
    let target = program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: signature(&[Type::I8], &[]),
    });
    let discarded = program.declare(signature(&[], &[]));
    let body = program.define(discarded).unwrap();
    let foreign = body.value::<I8>(7).unwrap();
    body.return_(()).unwrap();
    let run = program.declare(signature(&[], &[]));
    let mut body = program.define(run).unwrap();
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
    body.return_(()).unwrap();
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
    let mut body = program.define(run).unwrap();
    assert_eq!(
        body.if_(true, |arm| {
            assert_eq!(
                arm.return_(()),
                Err(BuildError::ResultCount {
                    expected: 1,
                    actual: 0
                })
            );
            Ok(())
        }),
        Err(BuildError::IncompleteBranch)
    );
    body.return_(7).unwrap();
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
}

#[test]
fn a_failed_no_result_body_rolls_back_its_helper_declarations() {
    let mut program = Program::new();
    let failure = program.function(signature(&[], &[]), |mut body| {
        let helper = body
            .program()
            .function(signature(&[], &[]), |body| body.return_(()))?;
        body.call::<()>(helper, &[7.into()])?;
        body.return_(())
    });
    assert_eq!(
        failure.err(),
        Some(BuildError::ArgumentCount {
            expected: 0,
            actual: 1
        })
    );
    let run = program
        .function(signature(&[], &[]), |body| body.return_(()))
        .unwrap();
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let bodies = Parser::new(0)
        .parse_all(&bytes)
        .filter(|part| matches!(part.as_ref().unwrap(), Payload::CodeSectionEntry(_)))
        .count();
    assert_eq!(bodies, 1);
}
