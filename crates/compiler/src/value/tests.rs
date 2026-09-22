mod arithmetic;
mod unbound;
mod visibility;

use super::{Val, ValueSource};
use crate::{
    BuildError, Func, FunctionImport, MemoryImport, Program, Signature, Type, I1, I32, I64, I8,
};

fn assert_closed(value: &Val<I32>) {
    for result in [
        value.add(0),
        value.mul(0),
        value.mul(1),
        value.unsigned().div(1),
        value.signed().div(1),
        value.unsigned().rem(1),
        value.signed().rem(1),
        Val::<I32>::from(0).mul(value),
        Val::<I32>::from(1).mul(value),
        Val::<I32>::from(0).and(value),
        value.popcnt(),
        value.clz(),
        value.ctz(),
    ] {
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
        shared: false,
    });
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
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
        results: vec![Type::I32],
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
        results: vec![Type::I64],
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
            results: vec![Type::I32],
        },
    });
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
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
        argument.resolve(arena, Type::I32, 0),
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
        argument.resolve(arena, Type::I32, 0),
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
        results: vec![Type::I32],
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
        shared: false,
    });
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
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
        results: vec![Type::I32],
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
        results: vec![Type::I32],
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
fn a_folded_result_still_checks_the_computed_count_owner() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
    });
    let discarded = program.define(function).unwrap();
    let foreign = discarded.parameter::<I32>(0).unwrap();
    drop(discarded);
    let body = program.define(function).unwrap();
    let zero = body.value::<I32>(0).unwrap();
    for result in [
        zero.shl(&foreign),
        zero.unsigned().shr(&foreign),
        zero.signed().shr(&foreign),
        zero.rotl(&foreign),
        zero.rotr(&foreign),
        body.value::<I1>(true)
            .unwrap()
            .rotl(&foreign)
            .unsigned()
            .extend::<I32>(),
        body.value::<I1>(false)
            .unwrap()
            .rotr(&foreign)
            .unsigned()
            .extend::<I32>(),
    ] {
        assert_eq!(body.value(result).err(), Some(BuildError::ForeignBody));
    }
    body.return_(zero).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn arithmetic_identity_folds_preserve_operand_errors() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    let discarded = program.define(function).unwrap();
    let foreign_zero = discarded.value::<I32>(0).unwrap();
    drop(discarded);
    let body = program.define(function).unwrap();
    let value = body.value::<I32>(7).unwrap();
    let failed = value.sub(&foreign_zero);
    for product in [
        value.mul(&foreign_zero),
        foreign_zero.mul(&value),
        failed.mul(0),
        failed.mul(1),
        Val::<I32>::from(0).mul(&failed),
        Val::<I32>::from(1).mul(&failed),
    ] {
        assert_eq!(body.value(product).err(), Some(BuildError::ForeignBody));
    }
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
    for count in [failed.popcnt(), failed.clz(), failed.ctz()] {
        assert_eq!(body.value(count).err(), Some(BuildError::ForeignBody));
    }
    body.return_(value.sub(0)).unwrap();
    assert!(program.compile().is_ok());
}
