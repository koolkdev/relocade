use super::{compile, signature};
use wasm86_compiler::{BuildError, FunctionImport, Program, Type, I8};
use wasmparser::{Parser, Payload};

#[test]
fn ordinary_and_tail_calls_require_the_declared_result_presence() {
    let mut program = Program::new();
    let void = program.import_function(FunctionImport {
        module: "test".into(),
        name: "void".into(),
        signature: signature(&[], None),
    });
    let value = program.import_function(FunctionImport {
        module: "test".into(),
        name: "value".into(),
        signature: signature(&[], Some(Type::I8)),
    });
    let run = program.declare(signature(&[], None));
    let mut body = program.define(run).unwrap();
    assert_eq!(
        body.call::<I8>(void, &[]).err(),
        Some(BuildError::MissingResult)
    );
    assert_eq!(
        body.call_void(value, &[]),
        Err(BuildError::UnexpectedResult)
    );
    assert_eq!(
        body.tail_call(value, &[]),
        Err(BuildError::UnexpectedResult)
    );
    let typed = program.declare(signature(&[], Some(Type::I8)));
    assert_eq!(
        program.define(typed).unwrap().tail_call(void, &[]),
        Err(BuildError::MissingResult)
    );
    program.define(typed).unwrap().return_(7).unwrap();
    program.define(run).unwrap().return_void().unwrap();
    let bytes = compile(program, run);
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|part| !matches!(part.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn invalid_returns_leave_the_function_undefined_and_can_be_retried() {
    let mut program = Program::new();
    let void = program.declare(signature(&[], None));
    assert_eq!(
        program.define(void).unwrap().return_(7),
        Err(BuildError::UnexpectedResult)
    );
    program.define(void).unwrap().return_void().unwrap();
    let value = program.declare(signature(&[], Some(Type::I8)));
    assert_eq!(
        program.define(value).unwrap().return_void(),
        Err(BuildError::MissingResult)
    );
    program.define(value).unwrap().return_(7).unwrap();
    compile(program, void);
}

#[test]
fn invalid_void_calls_validate_all_arguments_without_retaining_imports() {
    let mut program = Program::new();
    let target = program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: signature(&[Type::I8], None),
    });
    let discarded = program.declare(signature(&[], None));
    let body = program.define(discarded).unwrap();
    let foreign = body.value::<I8>(7).unwrap();
    body.return_void().unwrap();
    let run = program.declare(signature(&[], None));
    let mut body = program.define(run).unwrap();
    assert_eq!(
        body.call_void(target, &[]),
        Err(BuildError::ArgumentCount {
            expected: 1,
            actual: 0
        })
    );
    assert_eq!(
        body.call_void(target, &[7_u64.into()]),
        Err(BuildError::TypeMismatch {
            expected: Type::I8,
            actual: Type::I64
        })
    );
    assert_eq!(
        body.call_void(target, &[foreign.into()]),
        Err(BuildError::ForeignBody)
    );
    body.return_void().unwrap();
    let bytes = compile(program, run);
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|part| !matches!(part.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn an_ignored_invalid_void_return_cannot_become_a_fallthrough_branch() {
    let mut program = Program::new();
    let run = program.declare(signature(&[], Some(Type::I8)));
    let mut body = program.define(run).unwrap();
    assert_eq!(
        body.if_(true, |arm| {
            assert_eq!(arm.return_void(), Err(BuildError::MissingResult));
            Ok(())
        }),
        Err(BuildError::IncompleteBranch)
    );
    body.return_(7).unwrap();
    compile(program, run);
}

#[test]
fn a_failed_no_result_body_rolls_back_its_helper_declarations() {
    let mut program = Program::new();
    let failure = program.function(signature(&[], None), |mut body| {
        let helper = body
            .program()
            .function(signature(&[], None), |body| body.return_void())?;
        body.call_void(helper, &[7.into()])?;
        body.return_void()
    });
    assert_eq!(
        failure.err(),
        Some(BuildError::ArgumentCount {
            expected: 0,
            actual: 1
        })
    );
    let run = program
        .function(signature(&[], None), |body| body.return_void())
        .unwrap();
    let bytes = compile(program, run);
    let bodies = Parser::new(0)
        .parse_all(&bytes)
        .filter(|part| matches!(part.as_ref().unwrap(), Payload::CodeSectionEntry(_)))
        .count();
    assert_eq!(bodies, 1);
}
