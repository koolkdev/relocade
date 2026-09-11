use crate::{
    BuildError, FunctionBuilder, FunctionImport, Mem, MemoryImport, Program, Signature, Type, Val,
    I1, I32, I64,
};
use wasmparser::{Operator, Parser, Payload, Validator};

fn memory_program() -> (Program, Mem) {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
    });
    (program, memory)
}

fn child_constant(body: &mut FunctionBuilder<'_>, memory: Mem) -> Val<I32> {
    let mut retained = None;
    body.if_(false, |mut child| {
        let input = child.load::<I32>(memory, 0)?;
        retained = Some(child.value(input.and(0).add(1))?);
        Ok(())
    })
    .unwrap();
    retained.unwrap()
}

#[test]
fn fold_chains_keep_original_visibility_and_runtime_identity() {
    let (mut program, memory) = memory_program();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    let mut body = program.define(function).unwrap();
    let mut retained = vec![];
    body.if_(false, |mut child| {
        let input = child.load::<I32>(memory, 0)?;
        for (value, expected) in [
            (
                input
                    .and(0)
                    .add(1)
                    .unsigned()
                    .extend::<I64>()
                    .truncate::<I32>()
                    .popcnt(),
                1,
            ),
            (input.and(0).clz(), 32),
            (input.and(0).ctz(), 32),
            (input.mul(0).add(1), 1),
            (Val::<I32>::from(0).mul(&input).add(1), 1),
            (input.mul(1).and(0).add(1), 1),
            (Val::<I32>::from(1).mul(&input).and(0).add(1), 1),
            (input.or(-1).clz(), 0),
            (input.or(-1).ctz(), 0),
            (Val::<I32>::from(0).shl(&input).add(1), 1),
            (Val::<I32>::from(0).unsigned().shr(&input).add(1), 1),
            (Val::<I32>::from(0).signed().shr(&input).add(1), 1),
            (Val::<I32>::from(0).rotl(&input).add(1), 1),
            (Val::<I32>::from(0).rotr(&input).add(1), 1),
            (Val::<I32>::from(-1).rotl(&input).add(1), 0),
            (Val::<I32>::from(-1).rotr(&input).add(1), 0),
            (Val::<I32>::from(129).rotl(input.and(0)), 129),
            (Val::<I32>::from(129).rotr(input.and(0).add(32)), 129),
            (
                Val::<I1>::from(true)
                    .rotl(&input)
                    .unsigned()
                    .extend::<I32>(),
                1,
            ),
            (
                Val::<I1>::from(false)
                    .rotr(&input)
                    .unsigned()
                    .extend::<I32>(),
                0,
            ),
            (input.rotl(32).and(0).add(1), 1),
            (input.rotr(0).and(0).add(1), 1),
            (input.eq(&input).unsigned().extend::<I32>().add(1), 2),
            (Val::<I1>::from(false).select(&input, 7).add(1), 8),
            (input.ne(&input).select(&input, 8).add(1), 9),
        ] {
            let admitted = child.value(value)?;
            assert!(admitted.same_expression(&child.value::<I32>(expected)?));
            retained.push((admitted, expected));
        }
        Ok(())
    })
    .unwrap();
    for (value, expected) in retained {
        // Identity concerns the runtime node; admission still checks its provenance.
        assert!(value.same_expression(&body.value::<I32>(expected).unwrap()));
        assert_eq!(body.value(value).err(), Some(BuildError::OutOfScope));
    }
    body.return_(0).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn folded_operands_from_siblings_have_no_shared_visible_scope() {
    let (mut program, memory) = memory_program();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    let mut body = program.define(function).unwrap();
    let first = child_constant(&mut body, memory);
    body.if_(false, |mut sibling| {
        let local = sibling.load::<I32>(memory, 4)?.and(0).add(1);
        for value in [
            first.add(&local),
            first.mul(&local),
            first.rotl(&local),
            first.rotr(&local),
            first.eq(&local).unsigned().extend::<I32>(),
            Val::<I1>::from(true).select(&first, &local),
            Val::<I1>::from(false).select(&first, &local),
            first.eq(1).select::<I32>(7, 9),
        ] {
            assert_eq!(sibling.value(value).err(), Some(BuildError::OutOfScope));
        }
        Ok(())
    })
    .unwrap();
    body.return_(0).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn every_operand_and_argument_boundary_checks_folded_visibility() {
    let (mut program, memory) = memory_program();
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
    let mut body = program.define(function).unwrap();
    let value = child_constant(&mut body, memory);
    assert_eq!(body.value(&value).err(), Some(BuildError::OutOfScope));
    assert_eq!(body.store(memory, 0, &value), Err(BuildError::OutOfScope));
    assert_eq!(
        body.load_at::<I32>(memory, &value, 0).err(),
        Some(BuildError::OutOfScope)
    );
    assert_eq!(
        body.if_(value.eq(1), |_| Ok(())),
        Err(BuildError::OutOfScope)
    );
    assert_eq!(
        body.call::<I32>(target, &[value.argument()]).err(),
        Some(BuildError::OutOfScope)
    );
    assert_eq!(
        body.if_value::<I32>(true, |arm| arm.yield_(&value), |arm| arm.yield_(0))
            .err(),
        Some(BuildError::OutOfScope),
    );
    assert_eq!(
        body.block::<I32>(|block, exit| block.branch(&exit, value.argument()))
            .err(),
        Some(BuildError::OutOfScope),
    );
    body.return_(0).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn returns_and_tail_calls_check_folded_argument_visibility() {
    for tail_call in [false, true] {
        let (mut program, memory) = memory_program();
        let signature = Signature {
            parameters: vec![Type::I32],
            results: vec![Type::I32],
        };
        let target = program.import_function(FunctionImport {
            module: "test".into(),
            name: "target".into(),
            signature: signature.clone(),
        });
        let function = program.declare(signature);
        let mut body = program.define(function).unwrap();
        let value = child_constant(&mut body, memory);
        let error = if tail_call {
            body.tail_call(target, &[value.argument()])
        } else {
            body.return_(value.argument())
        };
        assert_eq!(error, Err(BuildError::OutOfScope));
        let body = program.define(function).unwrap();
        body.return_(0).unwrap();
        assert!(program.compile().is_ok());
    }
}

#[test]
fn legal_child_folds_discard_dead_loads_and_joins_gain_parent_visibility() {
    let (mut program, memory) = memory_program();
    program
        .function(
            Signature {
                parameters: vec![Type::I1],
                results: vec![Type::I32],
            },
            |mut body| {
                let predicate = body.parameter::<I1>(0)?;
                let joined = body.if_value::<I32>(
                    predicate,
                    |mut child| {
                        let input = child.load::<I32>(memory, 65536)?;
                        let zero = Val::<I32>::from(0);
                        let shifted = zero
                            .shl(&input)
                            .add(zero.unsigned().shr(&input))
                            .add(zero.signed().shr(&input))
                            .add(input.and(0))
                            .add(1);
                        let compared = input.eq(&input).unsigned().extend::<I32>();
                        let selected = Val::<I1>::from(false).select(&input, 9);
                        let folded = child.value(shifted.add(compared).add(selected))?;
                        assert!(folded.same_expression(&child.value::<I32>(11)?));
                        child.yield_(folded)
                    },
                    |child| child.yield_(4),
                )?;
                let admitted = body.value(joined)?;
                body.return_(admitted.add(1))
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut constants = vec![];
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operation in body.get_operators_reader().unwrap() {
                match operation.unwrap() {
                    Operator::I32Load { .. } => panic!("a discarded operand must not be read"),
                    Operator::I32Const { value } => constants.push(value),
                    _ => {}
                }
            }
        }
    }
    assert!(
        constants.contains(&11),
        "child calculations must still fold completely"
    );
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
        results: vec![Type::I32],
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
