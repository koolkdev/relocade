use super::{Val, ValueSource};
use crate::{
    BuildError, Func, FunctionImport, MemoryImport, Program, Signature, Type, I1, I32, I64, I8,
};

fn assert_closed(value: &Val<I32>) {
    for result in [value.add(0), Val::<I32>::from(0).and(value)] {
        assert!(matches!(
            result.source,
            ValueSource::Expression {
                expression: Err(BuildError::BodyClosed),
                ..
            }
        ));
    }
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
        result: Some(Type::I32),
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
        result: Some(Type::I32),
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
        result: Some(Type::I64),
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
            result: Some(Type::I32),
        },
    });
    let function = program.declare(Signature {
        parameters: vec![],
        result: Some(Type::I32),
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
    let ValueSource::Expression { arena, .. } = &value.source else {
        panic!("an admitted value retains its body");
    };
    assert_eq!(
        argument.resolve(arena, Type::I32),
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
    let ValueSource::Expression { arena, .. } = &value.source else {
        panic!("an admitted value retains its body");
    };
    assert_eq!(
        argument.resolve(arena, Type::I32),
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
        result: Some(Type::I32),
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
        result: Some(Type::I32),
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
fn equivalent_literals_share_admitted_expressions() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        result: Some(Type::I32),
    });
    let body = program.define(function).unwrap();
    let literal = Val::<I8>::from(-1);
    assert!(literal.same_expression(&Val::<I8>::from(0x1ff_u32)));
    let byte = body.value(&literal).unwrap();
    assert!(!literal.same_expression(&byte));
    assert!(byte.same_expression(&body.value(Val::<I8>::from(0x1ff_u32)).unwrap()));
    assert!(byte.same_expression(&body.value::<I8>(255).unwrap()));

    let signed = body.value(Val::<I64>::from(-1)).unwrap();
    assert!(signed.same_expression(&body.value(Val::<I64>::from(u64::MAX)).unwrap()));
    let unsigned = body.value(Val::<I64>::from(u32::MAX)).unwrap();
    assert!(unsigned.same_expression(&body.value(Val::<I64>::from(0xffff_ffff_u64)).unwrap()));
    assert!(!signed.same_expression(&unsigned));

    let input = body.parameter::<I32>(0).unwrap();
    let condition = Val::<I1>::from(false);
    assert!(condition.select(99, &input).same_expression(&input));
    body.return_(input).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn expression_identity_is_false_for_foreign_or_failed_values() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![],
        result: Some(Type::I32),
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

#[test]
fn a_zero_shift_still_checks_the_computed_count_owner() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        result: Some(Type::I32),
    });
    let discarded = program.define(function).unwrap();
    let foreign = discarded.parameter::<I32>(0).unwrap();
    drop(discarded);
    let body = program.define(function).unwrap();
    let zero = body.value::<I32>(0).unwrap();
    assert_eq!(
        body.value(zero.shl(foreign)).err(),
        Some(BuildError::ForeignBody)
    );
    body.return_(zero).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn constant_selection_still_checks_unused_operand_ownership_and_scope() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
    });
    let function = program.declare(Signature {
        parameters: vec![],
        result: Some(Type::I32),
    });
    let discarded = program.define(function).unwrap();
    let foreign = discarded.value::<I32>(9).unwrap();
    drop(discarded);
    let mut body = program.define(function).unwrap();
    assert_eq!(
        body.value(Val::<I1>::from(true).select(7, &foreign)).err(),
        Some(BuildError::ForeignBody)
    );
    assert_eq!(
        body.value(Val::<I1>::from(false).select(&foreign, 7)).err(),
        Some(BuildError::ForeignBody)
    );

    let mut sibling = None;
    body.if_(false, |mut branch| {
        sibling = Some(branch.load::<I32>(memory, 0)?);
        Ok(())
    })
    .unwrap();
    body.if_(false, |mut branch| {
        let local = branch.load::<I32>(memory, 4)?;
        let sibling = sibling.as_ref().unwrap();
        assert_eq!(
            branch
                .value(Val::<I1>::from(true).select(&local, sibling))
                .err(),
            Some(BuildError::OutOfScope)
        );
        assert_eq!(
            branch
                .value(Val::<I1>::from(false).select(sibling, &local))
                .err(),
            Some(BuildError::OutOfScope)
        );
        Ok(())
    })
    .unwrap();
    body.return_(7).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn arithmetic_identity_folds_preserve_operand_errors() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        result: Some(Type::I32),
    });
    let discarded = program.define(function).unwrap();
    let foreign_zero = discarded.value::<I32>(0).unwrap();
    drop(discarded);
    let body = program.define(function).unwrap();
    let value = body.value::<I32>(7).unwrap();
    let failed = value.sub(foreign_zero);
    assert_eq!(
        body.value(failed.sub(&failed)).err(),
        Some(BuildError::ForeignBody)
    );
    assert_eq!(
        body.value(failed.signed().ge(&failed)).err(),
        Some(BuildError::ForeignBody)
    );
    assert_eq!(
        body.value(Val::<I32>::from(0).and(&failed)).err(),
        Some(BuildError::ForeignBody)
    );
    body.return_(value.sub(0)).unwrap();
    assert!(program.compile().is_ok());
}
