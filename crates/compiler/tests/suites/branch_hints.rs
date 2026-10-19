use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, Observation, TestModule, Value};
use wasm86_compiler::{BranchCondition, BranchHint, BuildError, Type, Val, I1, I32};
use wasmparser::{BranchHint as EncodedHint, KnownCustom, Operator, Parser, Payload, Validator};

struct FunctionHints {
    function: u32,
    branches: Vec<EncodedHint>,
}

fn hints(bytes: &[u8]) -> Vec<FunctionHints> {
    Validator::new().validate_all(bytes).unwrap();
    let mut result = Vec::new();
    let mut sections = 0;
    let mut code_seen = false;
    let mut bodies = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::CustomSection(section) if section.name() == "metadata.code.branch_hint" => {
                assert!(!code_seen, "hints precede the code section");
                sections += 1;
                let KnownCustom::BranchHints(functions) = section.as_known() else {
                    panic!("branch hints have the standard encoding");
                };
                for function in functions {
                    let function = function.unwrap();
                    result.push(FunctionHints {
                        function: function.func,
                        branches: function.hints.into_iter().map(Result::unwrap).collect(),
                    });
                }
            }
            Payload::CodeSectionStart { .. } => code_seen = true,
            Payload::CodeSectionEntry(body) => bodies.push(body),
            _ => {}
        }
    }
    assert!(sections <= 1);
    assert!(result
        .windows(2)
        .all(|pair| pair[0].function < pair[1].function));
    // Function bodies are a suffix of the function index space. The difference
    // is the number of retained imports, independent of memory/table imports.
    let imported = Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::ImportSection(section) => Some(
                section
                    .into_iter()
                    .filter(|import| {
                        matches!(import.as_ref().unwrap().ty, wasmparser::TypeRef::Func(_))
                    })
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    for FunctionHints {
        function,
        branches: predictions,
    } in &result
    {
        assert!(!predictions.is_empty());
        assert!(predictions
            .windows(2)
            .all(|pair| pair[0].func_offset < pair[1].func_offset));
        let body = &bodies[*function as usize - imported];
        let branches: Vec<_> = body
            .get_operators_reader()
            .unwrap()
            .into_iter_with_offsets()
            .map(Result::unwrap)
            .filter_map(|(operator, offset)| {
                matches!(operator, Operator::If { .. } | Operator::BrIf { .. })
                    .then_some((offset - body.range().start) as u32)
            })
            .collect();
        for prediction in predictions {
            assert!(
                branches.contains(&prediction.func_offset),
                "hint at {} names a branch in function {function}",
                prediction.func_offset
            );
        }
    }
    result
}

fn conditional(hint: Option<BranchHint>, inverted: bool) -> TestModule {
    Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let predicate = if inverted { input.eq(0) } else { input.ne(0) };
        let condition = match hint {
            Some(BranchHint::Likely) => predicate.likely(),
            Some(BranchHint::Unlikely) => predicate.unlikely(),
            None => predicate.into(),
        };
        body.if_(condition, |arm| arm.return_(7))?;
        body.return_(11)
    })
}

fn code_bodies(bytes: &[u8]) -> Vec<&[u8]> {
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.as_bytes()),
            _ => None,
        })
        .collect()
}

#[test]
fn predictions_preserve_both_outcomes_and_condition_polarity() {
    for inverted in [false, true] {
        let ordinary = conditional(None, inverted);
        for hint in [None, Some(BranchHint::Likely), Some(BranchHint::Unlikely)] {
            let module = conditional(hint, inverted);
            assert_eq!(code_bodies(module.bytes()), code_bodies(ordinary.bytes()));
            let metadata = hints(module.bytes());
            if let Some(hint) = hint {
                assert_eq!(metadata.len(), 1);
                assert_eq!(metadata[0].branches.len(), 1);
                assert_eq!(metadata[0].branches[0].taken, hint == BranchHint::Likely);
            } else {
                assert!(metadata.is_empty());
            }
            for input in [0, 1] {
                assert_eq!(
                    module.instantiate().call::<i32>(input),
                    Ok(if (input != 0) != inverted { 7 } else { 11 })
                );
            }
        }
    }
}

#[test]
fn result_arms_and_conditional_edges_keep_their_own_predictions() {
    let module = Fixture::new().function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        let early = body.parameter::<I1>(1)?;
        let value = body.if_value::<I32>(
            choose.unlikely(),
            |mut arm| {
                arm.yield_if(early.likely(), 7)?;
                arm.yield_(11)
            },
            |mut arm| {
                let value = arm.block::<I32>(|mut block, exit| {
                    block.branch_if(early.unlikely(), &exit, 19)?;
                    block.yield_(23)
                })?;
                arm.yield_(value)
            },
        )?;
        body.return_(value)
    });
    assert_eq!(
        hints(module.bytes())[0]
            .branches
            .iter()
            .map(|hint| hint.taken)
            .collect::<Vec<_>>(),
        [false, true, false]
    );
    for (choose, early, expected) in [(0, 0, 23), (0, 1, 19), (1, 0, 11), (1, 1, 7)] {
        assert_eq!(
            module.instantiate().call::<i32>((choose, early)),
            Ok(expected)
        );
    }
}

#[test]
fn folded_discarded_and_unretained_branches_emit_no_hints() {
    let mut fixture = Fixture::new();
    fixture
        .program
        .function(signature(&[Type::I1], &[]), |mut body| {
            let choose = body.parameter::<I1>(0)?;
            body.if_(choose.likely(), |_| Ok(()))?;
            body.return_(())
        })
        .unwrap();
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        let failed = body.if_(choose.unlikely(), |_| Err(BuildError::UnknownParameter));
        assert_eq!(failed, Err(BuildError::UnknownParameter));
        body.if_(Val::<I1>::from(false).likely(), |mut arm| {
            arm.if_(choose.unlikely(), |inner| inner.trap())
        })?;
        body.if_(
            BranchCondition::from(true).with_hint(BranchHint::Unlikely),
            |_| Ok(()),
        )?;
        body.return_(17)
    });
    assert!(hints(module.bytes()).is_empty());
    assert_eq!(module.instantiate().call::<i32>(1), Ok(17));
}

#[test]
fn function_hints_use_retained_indices_after_imports() {
    let mut fixture = Fixture::new();
    fixture.callback("unused", signature(&[], &[]), &[]);
    let callback = fixture.callback("receive", signature(&[], &[]), &[]);
    let helper = fixture
        .program
        .function(signature(&[Type::I1], &[Type::I32]), |mut body| {
            let choose = body.parameter::<I1>(0)?;
            body.if_(choose.likely(), |arm| arm.return_(7))?;
            body.return_(11)
        })
        .unwrap();
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let choose = body.parameter::<I1>(0)?;
        body.call::<()>(callback, &[])?;
        body.if_else(
            choose.unlikely(),
            |arm| arm.tail_call(helper, &[true.into()]),
            |arm| arm.tail_call(helper, &[false.into()]),
        )?;
        body.trap()
    });
    let metadata = hints(module.bytes());
    assert_eq!(
        metadata
            .iter()
            .map(|function| function.function)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(metadata[0].branches[0].taken);
    assert!(!metadata[1].branches[0].taken);
    for (choose, expected) in [(0, 11), (1, 7)] {
        assert_eq!(module.instantiate().call::<i32>(choose), Ok(expected));
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn hinted_conditions_execute_in_v8() {
    for inverted in [false, true] {
        for hint in [BranchHint::Likely, BranchHint::Unlikely] {
            let module = conditional(Some(hint), inverted);
            for input in [0, 1] {
                let expected = if (input != 0) != inverted { 7 } else { 11 };
                assert_eq!(
                    module.run_v8(&Input::call("run", &[Value::I32(input)])),
                    Observation::returned(&[Value::I32(expected)])
                );
            }
        }
    }
}
