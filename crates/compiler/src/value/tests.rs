use super::Val;
use crate::{BuildError, Func, FunctionImport, MemoryImport, Program, Signature, Type, I32, I64};

fn assert_closed(value: &Val<I32>) {
    assert_eq!(value.add(0).expression, Err(BuildError::BodyClosed));
    assert_eq!(value.c::<I64>(0).expression, Err(BuildError::BodyClosed));
}

#[test]
fn returning_from_a_body_closes_retained_loads() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "state".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
    });
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I32,
    });
    let mut body = program.define(function).unwrap();
    let value = body.load::<I32>(memory, 0).unwrap();
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

fn tail_program() -> (Program, Func, Func) {
    let mut program = Program::new();
    let target = program.import_function(FunctionImport {
        module: "test".into(),
        name: "target".into(),
        signature: Signature {
            parameters: vec![Type::I32],
            result: Type::I32,
        },
    });
    let function = program.declare(Signature {
        parameters: vec![],
        result: Type::I32,
    });
    (program, function, target)
}

#[test]
fn a_tail_call_closes_retained_values_and_arguments() {
    let (mut program, function, target) = tail_program();
    let body = program.define(function).unwrap();
    let value = body.constant::<I32>(7);
    let argument = value.argument();
    body.tail_call(target, std::slice::from_ref(&argument))
        .unwrap();
    assert_closed(&value);
    assert_eq!(
        argument.admit(&value.arena, Type::I32),
        Err(BuildError::BodyClosed)
    );
    assert!(program.compile().is_ok());
}

#[test]
fn a_failed_tail_closes_its_values_without_retaining_the_import() {
    use wasmparser::{Parser, Payload};

    let (mut program, function, target) = tail_program();
    let discarded = program.define(function).unwrap();
    let foreign = discarded.constant::<I32>(0);
    drop(discarded);
    let body = program.define(function).unwrap();
    let value = body.constant::<I32>(7);
    let argument = value.add(&foreign).argument();
    assert_eq!(
        body.tail_call(target, std::slice::from_ref(&argument)),
        Err(BuildError::ForeignBody)
    );
    assert_closed(&value);
    assert_eq!(
        argument.admit(&value.arena, Type::I32),
        Err(BuildError::BodyClosed)
    );

    let body = program.define(function).unwrap();
    let result = body.constant::<I32>(7);
    body.return_(&result).unwrap();
    let bytes = program.compile().unwrap();
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
}
