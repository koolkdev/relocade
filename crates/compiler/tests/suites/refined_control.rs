//! Refined branches retain effects and exits without redundant result transport.

use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{FunctionImport, MemoryImport, Type, I1, I32};
use wasmparser::{Operator, Parser, Payload};

#[derive(Clone, Copy)]
enum Choice {
    Direct,
    Conditional,
    Switch,
}

fn tuple(choice: Choice) -> TestModule {
    Fixture::new().function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32; 2],
        |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let first = body.parameter::<I32>(1)?;
            let second = body.parameter::<I32>(2)?;
            body.if_(condition.eq(false), |arm| arm.return_((0, 0)))?;
            let result = match choice {
                Choice::Direct => (first, second),
                Choice::Conditional => body.if_value::<(I32, I32)>(
                    &condition,
                    |mut arm| {
                        let result = arm.if_value::<(I32, I32)>(
                            &condition,
                            |arm| arm.yield_((&first, &second)),
                            |arm| arm.trap(),
                        )?;
                        arm.yield_(result)
                    },
                    |arm| arm.trap(),
                )?,
                Choice::Switch => {
                    body.switch_value::<(I32, I32), I1>(condition, &[0, 1], |arm, key| match key {
                        Some(1) => arm.yield_((&first, &second)),
                        _ => arm.trap(),
                    })?
                }
            };
            body.return_(result)
        },
    )
}

fn operators(module: &TestModule) -> Vec<Operator<'_>> {
    Parser::new(0)
        .parse_all(module.bytes())
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body),
            _ => None,
        })
        .flat_map(|body| {
            body.get_operators_reader()
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
        })
        .collect()
}

#[test]
fn refined_conditional_and_switch_tuples_match_direct_code() {
    let direct = tuple(Choice::Direct);
    for choice in [Choice::Conditional, Choice::Switch] {
        let module = tuple(choice);
        assert_eq!(module.bytes(), direct.bytes());
        for (condition, expected) in [(0, (0, 0)), (1, (7, 11))] {
            assert_eq!(
                module.instantiate().call::<(i32, i32)>((condition, 7, 11)),
                Ok(expected)
            );
        }
    }
}

#[test]
fn helpers_used_only_by_a_removed_fallback_do_not_retain_functions_or_imports() {
    let mut fixture = Fixture::new();
    // No host definitions are supplied for these imports. The removed helper
    // chain is their only consumer.
    let memory = fixture.program.import_memory(MemoryImport {
        module: "test".into(),
        name: "dead_memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let called = fixture.program.import_function(FunctionImport {
        module: "test".into(),
        name: "dead_call".into(),
        signature: signature(&[], &[]),
    });
    let leaf = fixture
        .program
        .function(signature(&[], &[]), |mut body| {
            body.store::<I32>(memory, 0, 99)?;
            body.call::<()>(called, &[])?;
            body.return_(())
        })
        .unwrap();
    let helper = fixture
        .program
        .function(signature(&[], &[]), |mut body| {
            body.call::<()>(leaf, &[])?;
            body.return_(())
        })
        .unwrap();
    let module = fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        body.if_(condition.eq(false), |arm| arm.return_(0))?;
        let result = body.if_value::<I32>(
            condition,
            |arm| arm.yield_(7),
            |mut arm| {
                arm.call::<()>(helper, &[])?;
                arm.yield_(99)
            },
        )?;
        body.return_(result)
    });
    let mut functions = 0;
    for payload in Parser::new(0).parse_all(module.bytes()) {
        match payload.unwrap() {
            Payload::CodeSectionStart { count, .. } => functions = count,
            Payload::ImportSection(imports) => assert_eq!(imports.count(), 0),
            _ => {}
        }
    }
    assert_eq!(functions, 1);
    assert_eq!(module.instantiate().call::<i32>(1), Ok(7));
    assert_eq!(module.instantiate().call::<i32>(0), Ok(0));
}

fn snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        body.if_(condition.eq(false), |arm| arm.return_(0))?;
        let result = body.if_value::<I32>(
            condition,
            |mut arm| {
                let before = arm.load::<I32>(state, 0)?;
                arm.yield_(before)
            },
            |arm| arm.trap(),
        )?;
        body.store::<I32>(state, 0, 11)?;
        body.return_(result)
    })
}

#[test]
fn forwarding_a_selected_result_keeps_its_read_before_a_later_write() {
    let module = snapshot();
    let ops = operators(&module);
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::LocalSet { .. }))
            .count(),
        1
    );
    assert!(!ops.iter().any(|op| matches!(op, Operator::Block { .. })));
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(1), Ok(7));
    assert_eq!(&instance.memory("state")[..4], &[11, 0, 0, 0]);
}

fn early_result() -> TestModule {
    Fixture::new().function(&[Type::I1; 2], &[Type::I32], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let early = body.parameter::<I1>(1)?;
        body.if_(condition.eq(false), |arm| arm.return_(0))?;
        let result = body.if_value::<I32>(
            condition,
            |mut arm| {
                arm.yield_if(early, 7)?;
                arm.yield_(11)
            },
            |arm| arm.trap(),
        )?;
        body.return_(result)
    })
}

#[test]
fn a_selected_arm_keeps_the_label_needed_by_an_early_result() {
    let module = early_result();
    assert!(operators(&module)
        .iter()
        .any(|op| matches!(op, Operator::Block { .. })));
    for (condition, early, expected) in [(0, 0, 0), (1, 0, 11), (1, 1, 7)] {
        assert_eq!(
            module.instantiate().call::<i32>((condition, early)),
            Ok(expected)
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn refined_results_preserve_snapshots_and_early_exits_in_v8() {
    for choice in [Choice::Conditional, Choice::Switch] {
        assert_eq!(
            tuple(choice).run_v8(&Input::call(
                "run",
                &[Value::I32(1), Value::I32(7), Value::I32(11)]
            )),
            Observation::returned(&[Value::I32(7), Value::I32(11)])
        );
    }
    assert_eq!(
        snapshot().run_v8(
            &Input::call("run", &[Value::I32(1)])
                .with_memories(&[MemoryBytes::new("state", &[7, 0, 0, 0])])
        ),
        Observation::returned(&[Value::I32(7)])
            .with_memories(&[MemoryBytes::new("state", &[11, 0, 0, 0])])
    );
    for (early, expected) in [(0, 11), (1, 7)] {
        assert_eq!(
            early_result().run_v8(&Input::call("run", &[Value::I32(1), Value::I32(early)])),
            Observation::returned(&[Value::I32(expected)])
        );
    }
}
