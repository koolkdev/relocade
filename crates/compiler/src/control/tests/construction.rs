use std::cell::Cell;

use crate::{BuildError, FunctionImport, Program, Signature, Type, I1, I32};

fn signature() -> Signature {
    Signature {
        parameters: vec![Type::I1, Type::I1],
        results: vec![Type::I32],
    }
}

#[test]
fn constant_conditions_invoke_only_the_reachable_closure() {
    for condition in [false, true] {
        let mut program = Program::new();
        let function = program
            .function(signature(), |mut body| {
                body.if_(false, |_| panic!("a skipped closure must not run"))?;
                let calls = Cell::new(0);
                body.if_else(
                    condition,
                    |_| {
                        assert!(condition);
                        calls.set(calls.get() + 1);
                        Ok(())
                    },
                    |_| {
                        assert!(!condition);
                        calls.set(calls.get() + 1);
                        Ok(())
                    },
                )?;
                let value = body.if_value::<I32>(
                    condition,
                    |arm| {
                        assert!(condition);
                        calls.set(calls.get() + 1);
                        arm.yield_(7)
                    },
                    |arm| {
                        assert!(!condition);
                        calls.set(calls.get() + 1);
                        arm.yield_(11)
                    },
                )?;
                assert_eq!(calls.get(), 2);
                body.return_(value)
            })
            .unwrap();
        program.export("run", function).unwrap();
        wasmparser::Validator::new()
            .validate_all(&program.compile().unwrap())
            .unwrap();
    }
}

#[test]
fn returning_and_tail_calling_guards_settle_the_continuing_path() {
    for truth in [false, true] {
        for tail_call in [false, true] {
            let mut program = Program::new();
            let tail = program.import_function(FunctionImport {
                module: "test".into(),
                name: "tail".into(),
                signature: Signature {
                    parameters: vec![],
                    results: vec![Type::I32],
                },
            });
            let function = program
                .function(signature(), |mut body| {
                    let condition = body.parameter::<I1>(0)?;
                    body.if_(condition.eq(!truth), |arm| {
                        if tail_call {
                            arm.tail_call(tail, &[])
                        } else {
                            arm.return_(3)
                        }
                    })?;
                    let result = body.if_value::<I32>(
                        &condition,
                        |arm| {
                            assert!(truth);
                            arm.yield_(7)
                        },
                        |arm| {
                            assert!(!truth);
                            arm.yield_(11)
                        },
                    )?;
                    body.if_(condition.eq(!truth), |_| panic!("the guard already exited"))?;
                    body.if_(
                        condition.unsigned().extend::<I32>().eq(u32::from(!truth)),
                        |_| panic!("a carrier-preserving view keeps the same decision"),
                    )?;
                    body.return_(result)
                })
                .unwrap();
            program.export("run", function).unwrap();
            wasmparser::Validator::new()
                .validate_all(&program.compile().unwrap())
                .unwrap();
        }
    }
}

#[test]
fn dynamic_arms_inherit_their_decision_without_refining_their_sibling_or_parent() {
    let mut program = Program::new();
    program
        .function(signature(), |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let calls = Cell::new(0);
            body.if_else(
                &condition,
                |mut arm| {
                    calls.set(calls.get() + 1);
                    arm.if_(condition.eq(false), |_| {
                        panic!("contradicts the taken edge")
                    })
                },
                |mut arm| {
                    calls.set(calls.get() + 1);
                    arm.if_(&condition, |_| panic!("contradicts the other edge"))
                },
            )?;
            body.if_else(
                condition,
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
            )?;
            assert_eq!(calls.get(), 4);
            body.return_(0)
        })
        .unwrap();
}

#[test]
fn a_failed_conditional_discards_its_path_decisions() {
    let mut program = Program::new();
    program
        .function(signature(), |mut body| {
            let first = body.parameter::<I1>(0)?;
            let second = body.parameter::<I1>(1)?;
            assert_eq!(
                body.if_else(
                    &first,
                    |mut arm| {
                        arm.if_(&second, |guard| guard.return_(1))?;
                        Err(BuildError::UnknownParameter)
                    },
                    |_| Ok(()),
                ),
                Err(BuildError::UnknownParameter)
            );
            let calls = Cell::new(0);
            for condition in [first, second] {
                body.if_else(
                    condition,
                    |_| {
                        calls.set(calls.get() + 1);
                        Ok(())
                    },
                    |_| {
                        calls.set(calls.get() + 1);
                        Ok(())
                    },
                )?;
            }
            assert_eq!(calls.get(), 4);
            body.return_(0)
        })
        .unwrap();
}

#[test]
fn outward_guards_refine_only_the_body_they_leave() {
    let mut program = Program::new();
    program
        .function(signature(), |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let value = body.block::<I32>(|mut block, exit| {
                block.branch_if(&condition, &exit, 7)?;
                block.if_(&condition, |_| panic!("the outward branch already exited"))?;
                block.yield_(11)
            })?;
            let calls = Cell::new(0);
            body.if_else(
                condition,
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
            )?;
            assert_eq!(calls.get(), 2);
            body.return_(value)
        })
        .unwrap();
}

#[test]
fn a_typed_join_does_not_inherit_one_yielding_arms_decisions() {
    let mut program = Program::new();
    program
        .function(signature(), |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let value =
                body.if_value::<I32>(&condition, |arm| arm.yield_(7), |arm| arm.yield_(11))?;
            let calls = Cell::new(0);
            body.if_else(
                condition,
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Ok(())
                },
            )?;
            assert_eq!(calls.get(), 2);
            body.return_(value)
        })
        .unwrap();
}

#[test]
fn constant_switches_build_only_the_matching_key_or_default_but_validate_all_keys() {
    for selector in [2, 3] {
        let mut program = Program::new();
        program
            .function(signature(), |mut body| {
                let expected = (selector == 2).then_some(2);
                let mut calls = 0;
                body.switch::<I32>(selector, &[2, 5], |_, key| {
                    assert_eq!(key, expected);
                    calls += 1;
                    Ok(())
                })?;
                let result = body.switch_value::<I32, I32>(selector, &[2, 5], |arm, key| {
                    assert_eq!(key, expected);
                    calls += 1;
                    arm.yield_(17)
                })?;
                assert_eq!(calls, 2);
                assert_eq!(
                    body.switch::<I32>(selector, &[5, 5], |_, _| panic!("invalid keys")),
                    Err(BuildError::DuplicateSwitchCase { key: 5 })
                );
                body.return_(result)
            })
            .unwrap();
    }
}
