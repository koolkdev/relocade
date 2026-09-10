use super::*;

fn no_imports(bytes: &[u8]) {
    Validator::new().validate_all(bytes).unwrap();
    assert!(Parser::new(0)
        .parse_all(bytes)
        .all(|part| !matches!(part.unwrap(), Payload::ImportSection(_))));
}

#[test]
fn rejected_multi_result_calls_leave_the_caller_open_and_do_not_retain_imports() {
    let mut program = Program::new();
    let target = program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: signature(&[Type::I32, Type::I32], &[Type::I1, Type::I8, Type::I64]),
    });
    let other = program.declare(signature(&[], &[Type::I32]));
    let foreign_body = program.define(other).unwrap();
    let foreign = foreign_body.value::<I32>(7).unwrap();
    foreign_body.return_(0).unwrap();
    let run = program.declare(signature(&[], &[Type::I32]));
    let mut body = program.define(run).unwrap();
    let arguments = [7.into(), 11.into()];
    assert_eq!(
        body.call::<(I1, I8)>(target, &arguments).err(),
        Some(BuildError::ResultCount {
            expected: 2,
            actual: 3
        })
    );
    assert_eq!(
        body.call::<()>(target, &arguments).err(),
        Some(BuildError::ResultCount {
            expected: 0,
            actual: 3
        })
    );
    assert_eq!(
        body.call::<(I1, I16, I64)>(target, &arguments).err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I16,
            actual: Type::I8
        })
    );
    assert_eq!(
        body.call::<(I8, I1, I64)>(target, &arguments).err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I8,
            actual: Type::I1
        })
    );
    assert_eq!(
        body.call::<(I1, I8, I64)>(target, &[7.into()]).err(),
        Some(BuildError::ArgumentCount {
            expected: 2,
            actual: 1
        })
    );
    assert_eq!(
        body.call::<(I1, I8, I64)>(target, &[7.into(), Val::<I8>::from(11).into()])
            .err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I32,
            actual: Type::I8
        })
    );
    assert_eq!(
        body.call::<(I1, I8, I64)>(target, &[7.into(), foreign.into()])
            .err(),
        Some(BuildError::ForeignBody)
    );
    body.return_(23).unwrap();
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    no_imports(&bytes);
    assert_eq!(
        TestModule::new(&bytes)
            .instantiate()
            .call::<i32>(())
            .unwrap(),
        23
    );
}

#[test]
fn every_call_component_retains_its_branch_visibility_even_after_folding() {
    let mut program = Program::new();
    let helper = program
        .function(signature(&[], &[Type::I8, Type::I64]), |body| {
            body.return_((7, 11_u64))
        })
        .unwrap();
    let run = program.declare(signature(&[], &[Type::I32]));
    let mut body = program.define(run).unwrap();
    let mut escaped = None;
    body.if_(false, |mut arm| {
        escaped = Some(arm.call::<(I8, I64)>(helper, &[])?);
        Ok(())
    })
    .unwrap();
    let (byte, wide) = escaped.unwrap();
    assert_eq!(body.value(byte.add(1)).err(), Some(BuildError::OutOfScope));
    assert_eq!(body.value(byte.and(0)).err(), Some(BuildError::OutOfScope));
    assert_eq!(body.value(wide.add(1)).err(), Some(BuildError::OutOfScope));
    assert_eq!(body.value(wide.and(0)).err(), Some(BuildError::OutOfScope));
    body.return_(23).unwrap();
    program.export("run", run).unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    assert_eq!(inspect(&module, "run").calls, 0);
    assert_eq!(module.instantiate().call::<i32>(()).unwrap(), 23);
}

#[test]
fn discarded_regions_do_not_retain_multi_result_calls_or_poison_parent_placement() {
    for imported in [false, true] {
        for later_arm in [false, true] {
            let mut program = Program::new();
            let shape = signature(&[], &[Type::I32, Type::I64]);
            let target = if imported {
                program.import_function(FunctionImport {
                    module: "test".into(),
                    name: "receive".into(),
                    signature: shape,
                })
            } else {
                program
                    .function(shape, |body| body.return_((7, 11_u64)))
                    .unwrap()
            };
            let run = program.declare(signature(&[], &[Type::I32]));
            let mut body = program.define(run).unwrap();
            let error = if later_arm {
                body.if_value::<I32>(
                    true,
                    |mut arm| {
                        let (first, _second) = arm.call::<(I32, I64)>(target, &[])?;
                        arm.yield_(first)
                    },
                    |_arm| Err(BuildError::UnknownParameter),
                )
                .err()
            } else {
                body.if_(true, |mut arm| {
                    let _unused = arm.call::<(I32, I64)>(target, &[])?;
                    Err(BuildError::UnknownParameter)
                })
                .err()
            };
            assert_eq!(error, Some(BuildError::UnknownParameter));
            body.return_(23).unwrap();
            program.export("run", run).unwrap();
            let bytes = program.compile().unwrap();
            no_imports(&bytes);
            let module = TestModule::new(&bytes);
            assert_eq!(inspect(&module, "run").calls, 0);
            assert_eq!(module.instantiate().call::<i32>(()).unwrap(), 23);
        }
    }
}

#[test]
fn invalid_multi_returns_leave_the_function_undefined_for_a_fresh_definition() {
    let mut program = Program::new();
    let helper = program.declare(signature(&[], &[Type::I64]));
    let other = program.define(helper).unwrap();
    let foreign = other.value::<I64>(7_u64).unwrap();
    other.return_(11_u64).unwrap();
    let run = program.declare(signature(&[], &[Type::I1, Type::I8, Type::I64]));
    assert_eq!(
        program.define(run).unwrap().return_(true),
        Err(BuildError::ResultCount {
            expected: 3,
            actual: 1
        })
    );
    assert_eq!(
        program.define(run).unwrap().return_(()),
        Err(BuildError::ResultCount {
            expected: 3,
            actual: 0
        })
    );
    assert_eq!(
        program.define(run).unwrap().return_((true, 255, 7_u64, 9)),
        Err(BuildError::ResultCount {
            expected: 3,
            actual: 4
        })
    );
    assert_eq!(
        program
            .define(run)
            .unwrap()
            .return_((true, Val::<I16>::from(255), 7_u64)),
        Err(BuildError::TypeMismatch {
            expected: Type::I8,
            actual: Type::I16
        })
    );
    assert_eq!(
        program.define(run).unwrap().return_((true, 255, foreign)),
        Err(BuildError::ForeignBody)
    );
    let mut body = program.define(run).unwrap();
    let mut escaped = None;
    body.if_(false, |mut arm| {
        escaped = Some(arm.call::<I64>(helper, &[])?);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        body.return_((true, 255, escaped.unwrap().and(0))),
        Err(BuildError::OutOfScope)
    );
    program
        .define(run)
        .unwrap()
        .return_((true, 511, u64::MAX))
        .unwrap();
    program.export("run", run).unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    assert_eq!(
        module.instantiate().call::<(i32, i32, i64)>(()).unwrap(),
        (1, 255, -1)
    );
}

#[test]
fn an_ignored_invalid_multi_return_does_not_turn_into_branch_fallthrough() {
    let mut program = Program::new();
    let run = program.declare(signature(&[], &[Type::I1, Type::I8]));
    let mut body = program.define(run).unwrap();
    assert_eq!(
        body.if_(true, |arm| {
            assert_eq!(
                arm.return_(true),
                Err(BuildError::ResultCount {
                    expected: 2,
                    actual: 1
                })
            );
            Ok(())
        }),
        Err(BuildError::IncompleteBranch)
    );
    body.return_((false, 7)).unwrap();
    program.export("run", run).unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    assert_eq!(module.instantiate().call::<(i32, i32)>(()).unwrap(), (0, 7));
}

#[test]
fn tail_calls_compare_the_complete_ordered_logical_result_signature() {
    for (results, expected) in [
        (
            vec![],
            BuildError::ResultCount {
                expected: 3,
                actual: 0,
            },
        ),
        (
            vec![Type::I1],
            BuildError::ResultCount {
                expected: 3,
                actual: 1,
            },
        ),
        (
            vec![Type::I8, Type::I1, Type::I64],
            BuildError::TypeMismatch {
                expected: Type::I1,
                actual: Type::I8,
            },
        ),
        (
            vec![Type::I1, Type::I8, Type::I32],
            BuildError::TypeMismatch {
                expected: Type::I64,
                actual: Type::I32,
            },
        ),
    ] {
        let mut program = Program::new();
        let target = program.import_function(FunctionImport {
            module: "test".into(),
            name: "receive".into(),
            signature: signature(&[], &results),
        });
        let run = program.declare(signature(&[], &[Type::I1, Type::I8, Type::I64]));
        assert_eq!(
            program.define(run).unwrap().tail_call(target, &[]),
            Err(expected)
        );
        program
            .define(run)
            .unwrap()
            .return_((true, 7, 11_u64))
            .unwrap();
        program.export("run", run).unwrap();
        let bytes = program.compile().unwrap();
        no_imports(&bytes);
        assert_eq!(
            TestModule::new(&bytes)
                .instantiate()
                .call::<(i32, i32, i64)>(())
                .unwrap(),
            (1, 7, 11)
        );
    }
}
