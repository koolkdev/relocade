use super::Val;
use crate::{BuildError, Func, FunctionImport, MemoryImport, Program, Signature, Type, I32};

fn assert_closed(value: &Val<I32>) {
    assert_eq!(value.add(0).expression, Err(BuildError::BodyClosed));
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
    let value = body.value::<I32>(7).unwrap();
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
    let value = body.value::<I32>(7).unwrap();
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
    let value = body.value::<I32>(7).unwrap();
    let argument = value.argument();
    body.tail_call(target, std::slice::from_ref(&argument))
        .unwrap();
    assert_closed(&value);
    assert_eq!(
        argument.resolve(&value.arena, Type::I32),
        Err(BuildError::BodyClosed)
    );
    assert!(program.compile().is_ok());
}

#[test]
fn a_failed_tail_closes_its_values_without_retaining_the_import() {
    use wasmparser::{Parser, Payload};

    let (mut program, function, target) = tail_program();
    let discarded = program.define(function).unwrap();
    let foreign = discarded.value::<I32>(0).unwrap();
    drop(discarded);
    let body = program.define(function).unwrap();
    let value = body.value::<I32>(7).unwrap();
    let argument = value.add(&foreign).argument();
    assert_eq!(
        body.tail_call(target, std::slice::from_ref(&argument)),
        Err(BuildError::ForeignBody)
    );
    assert_closed(&value);
    assert_eq!(
        argument.resolve(&value.arena, Type::I32),
        Err(BuildError::BodyClosed)
    );

    let body = program.define(function).unwrap();
    body.return_(7).unwrap();
    let bytes = program.compile().unwrap();
    assert!(Parser::new(0)
        .parse_all(&bytes)
        .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn retaining_a_failed_expression_leaves_the_body_usable() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        result: Type::I32,
    });
    let discarded = program.define(function).unwrap();
    let foreign_zero = discarded.value::<I32>(0).unwrap();
    drop(discarded);

    let body = program.define(function).unwrap();
    let value = body.parameter::<I32>(0).unwrap();
    assert_eq!(
        body.value(value.add(&foreign_zero)).err(),
        Some(BuildError::ForeignBody)
    );
    let retained = body.value(&value).unwrap();
    body.return_(retained.add(1)).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn expression_identity_reuses_nodes_but_keeps_read_events_distinct() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    });
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        result: Type::I32,
    });
    let mut body = program.define(function).unwrap();
    let input = body.parameter::<I32>(0).unwrap();
    let sum = input.add(1);
    assert!(sum.same_expression(&sum.clone()));
    assert!(sum.same_expression(&input.add(1)));
    let first = body.load::<I32>(memory, 0).unwrap();
    let second = body.load::<I32>(memory, 0).unwrap();
    assert!(!first.same_expression(&second));
    body.return_(&sum).unwrap();
    assert!(sum.same_expression(&sum.clone()));
}

#[test]
fn expression_identity_is_false_for_foreign_or_failed_values() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![],
        result: Type::I32,
    };
    let first = program.declare(signature.clone());
    let second = program.declare(signature);
    let body = program.define(first).unwrap();
    let foreign = body.value::<I32>(7).unwrap();
    body.return_(&foreign).unwrap();
    let body = program.define(second).unwrap();
    let current = body.value::<I32>(7).unwrap();
    assert!(!current.same_expression(&foreign));
    let failed = current.add(&foreign);
    assert!(!failed.same_expression(&failed));
    assert!(!failed.same_expression(&current));
    assert_eq!(body.value(&foreign).err(), Some(BuildError::ForeignBody));
    body.return_(current).unwrap();
}
