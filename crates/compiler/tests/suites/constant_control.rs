use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, Callback, Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{FunctionImport, MemoryImport, Type, Val, I1, I32};
use wasmparser::{Operator, Parser, Payload, Validator};

fn operators(module: &TestModule) -> Vec<Operator<'_>> {
    Validator::new().validate_all(module.bytes()).unwrap();
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

fn assert_no_conditional(module: &TestModule) {
    assert!(!operators(module).iter().any(|op| matches!(
        op,
        Operator::If { .. } | Operator::Else | Operator::BrIf { .. } | Operator::Nop
    )));
}

fn removed_effects() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    // These imports deliberately have no host definitions. Their only consumers
    // are in removed arms, including call results left in the expression arena.
    let dead_memory = fixture.program.import_memory(MemoryImport {
        module: "test".into(),
        name: "dead_memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let dead_call = fixture.program.import_function(FunctionImport {
        module: "test".into(),
        name: "dead_call".into(),
        signature: signature(&[], &[Type::I32]),
    });
    let live = fixture.callback(
        "live",
        signature(&[Type::I32], &[Type::I32]),
        &[Value::I32(23)],
    );
    fixture.function(&[Type::I32], &[Type::I32, Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        body.if_(false, |mut arm| {
            let called = arm.call::<I32>(dead_call, &[])?;
            arm.store(dead_memory, 0, called)?;
            arm.trap()
        })?;
        let before = body.load::<I32>(state, 0)?;
        body.if_(true, |mut arm| arm.store::<I32>(state, 0, 11))?;
        let called = body.call::<I32>(live, &[before.argument()])?;
        let joined = body.if_value::<I32>(
            input.eq(&input),
            |arm| arm.yield_(before.add(called)),
            |mut arm| {
                arm.call::<I32>(dead_call, &[])?;
                let unreachable = arm.load::<I32>(dead_memory, 65536)?;
                arm.yield_(unreachable)
            },
        )?;
        let after = body.load::<I32>(state, 0)?;
        body.return_((joined, after))
    })
}

#[test]
fn dead_arms_remove_imports_and_demands_without_moving_later_producer_sites() {
    let module = removed_effects();
    assert_no_conditional(&module);
    let imports: Vec<_> = Parser::new(0)
        .parse_all(module.bytes())
        .filter_map(|payload| match payload.unwrap() {
            Payload::ImportSection(imports) => Some(imports),
            _ => None,
        })
        .flatten()
        .map(|import| import.unwrap().name)
        .collect();
    assert_eq!(imports, ["live", "state"]);
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<(i32, i32)>(91), Ok((30, 11)));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("live", &[Value::I32(7)])
            .with_memories(&[MemoryBytes::new("state", &[11, 0, 0, 0])])]
    );
}

#[test]
fn removed_writes_do_not_force_an_unused_helper_call() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let helper = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut body| {
            body.if_(false, |mut arm| arm.store::<I32>(state, 0, 99))?;
            body.return_(23)
        })
        .unwrap();
    let module = fixture.function(&[], &[Type::I32], |mut body| {
        let before = body.load::<I32>(state, 0)?;
        body.call::<I32>(helper, &[])?;
        body.store::<I32>(state, 0, 11)?;
        body.return_(before)
    });
    assert_no_conditional(&module);
    assert!(!operators(&module)
        .iter()
        .any(|op| matches!(op, Operator::Call { .. })));
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()), Ok(7));
    assert_eq!(&instance.memory("state")[..4], &[11, 0, 0, 0]);
}

fn alternative_stores(condition: Option<bool>) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0, 0, 0, 0]);
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
        let condition = match condition {
            Some(value) => Val::<I1>::from(value),
            None => body.parameter::<I1>(0)?,
        };
        body.if_else(
            condition,
            |mut arm| arm.store::<I32>(state, 0, 7),
            |mut arm| arm.store::<I32>(state, 0, 11),
        )?;
        let value = body.load::<I32>(state, 0)?;
        body.return_(value)
    })
}

#[test]
fn constant_effectful_arms_emit_inline_and_dynamic_conditions_keep_their_if() {
    for (condition, expected) in [(false, 11), (true, 7)] {
        let module = alternative_stores(Some(condition));
        assert_no_conditional(&module);
        assert!(!operators(&module)
            .iter()
            .any(|op| matches!(op, Operator::Block { .. })));
        assert_eq!(module.instantiate().call::<i32>(0), Ok(expected));
    }
    let dynamic = alternative_stores(None);
    assert_eq!(
        operators(&dynamic)
            .iter()
            .filter(|op| matches!(op, Operator::If { .. }))
            .count(),
        1
    );
    assert_eq!(dynamic.instantiate().call::<i32>(0), Ok(11));
    assert_eq!(dynamic.instantiate().call::<i32>(1), Ok(7));
}

fn outward_exit(taken: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0, 0, 0, 0]);
    fixture.function(&[], &[Type::I32], |mut body| {
        let value = body.block::<I32>(|mut outer, exit| {
            outer.if_(true, |mut arm| {
                arm.if_(true, |mut inner| {
                    inner.branch_if(taken, &exit, 7)?;
                    inner.store::<I32>(state, 0, 1)
                })?;
                arm.store::<I32>(state, 0, 2)
            })?;
            outer.store::<I32>(state, 0, 3)?;
            outer.yield_if(taken, 13)?;
            outer.yield_(11)
        })?;
        body.return_(value)
    })
}

#[test]
fn folded_outward_exits_keep_their_target_and_skip_all_parent_continuations() {
    for (taken, expected, stored) in [(false, 11, 3), (true, 7, 0)] {
        let module = outward_exit(taken);
        assert_no_conditional(&module);
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(()), Ok(expected));
        assert_eq!(&instance.memory("state")[..4], &[stored, 0, 0, 0]);
    }
}

fn unconditional_backedge() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0, 0, 0, 0]);
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let result =
            body.loop_::<(I32, I32), I32>((count, 0), |mut iteration, labels, (left, sum)| {
                iteration.branch_if(left.eq(0), &labels.exit, &sum)?;
                iteration.branch_if(true, &labels.again, (left.sub(1), sum.add(7)))?;
                iteration.store::<I32>(state, 0, 99)?;
                iteration.yield_(0)
            })?;
        body.return_(result)
    })
}

#[test]
fn a_constant_backedge_keeps_loop_inputs_and_skips_later_effects() {
    let module = unconditional_backedge();
    operators(&module);
    for (count, expected) in [(0, 0), (1, 7), (3, 21)] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(count), Ok(expected));
        assert_eq!(&instance.memory("state")[..4], &[0; 4]);
    }
}

#[derive(Clone, Copy)]
enum Exit {
    Return,
    TailCall,
    Trap,
}

fn terminating_result(exit: Exit, condition: bool, observe: bool) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0, 0, 0, 0]);
    let tail = fixture.callback("tail", signature(&[], &[Type::I32]), &[Value::I32(19)]);
    fixture.function(&[], &[Type::I32], |mut body| {
        let arm = |mut arm: wasm86_compiler::FunctionBuilder<'_>, taken: bool| {
            if !taken {
                return arm.yield_(23);
            }
            arm.store::<I32>(state, 0, 1)?;
            match exit {
                Exit::Return => arm.return_(7),
                Exit::TailCall => arm.tail_call(tail, &[]),
                Exit::Trap => arm.trap(),
            }
        };
        let result = body.if_value::<I32>(
            condition,
            |branch| arm(branch, condition),
            |branch| arm(branch, !condition),
        )?;
        body.store::<I32>(state, 0, 99)?;
        body.return_(if observe { result } else { 41.into() })
    })
}

#[test]
fn selected_function_exits_preserve_live_and_unused_result_joins() {
    for exit in [Exit::Return, Exit::TailCall, Exit::Trap] {
        for condition in [false, true] {
            for observe in [false, true] {
                let module = terminating_result(exit, condition, observe);
                assert_no_conditional(&module);
                let mut instance = module.instantiate();
                let expected = match exit {
                    Exit::Return => Ok(7),
                    Exit::TailCall => Ok(19),
                    Exit::Trap => Err(wasmtime::Trap::UnreachableCodeReached),
                };
                assert_eq!(instance.call::<i32>(()), expected);
                assert_eq!(&instance.memory("state")[..4], &[1, 0, 0, 0]);
                assert_eq!(
                    instance.callbacks().len(),
                    usize::from(matches!(exit, Exit::TailCall))
                );
            }
        }
    }
}

#[test]
fn enclosing_results_are_validated_before_their_only_authored_edge_is_removed() {
    let module = Fixture::new().function(&[], &[Type::I32], |mut body| {
        let result = body.block::<I32>(|mut block, exit| {
            block.if_(false, |arm| arm.branch(&exit, 23))?;
            block.return_(7)
        })?;
        body.return_(result)
    });
    assert_no_conditional(&module);
    assert_eq!(module.instantiate().call::<i32>(()), Ok(7));
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn folded_constants_preserve_effects_results_and_backedges_in_v8() {
    assert_eq!(
        removed_effects().run_v8(
            &Input::call("run", &[Value::I32(91)])
                .with_memories(&[MemoryBytes::new("state", &[7, 0, 0, 0])])
                .with_callbacks(&[Callback::new("live", &[Value::I32(23)])])
        ),
        Observation::returned(&[Value::I32(30), Value::I32(11)])
            .with_memories(&[MemoryBytes::new("state", &[11, 0, 0, 0])])
            .with_callbacks(&[Call::new("live", &[Value::I32(7)])
                .with_memories(&[MemoryBytes::new("state", &[11, 0, 0, 0])])])
    );
    for taken in [false, true] {
        assert_eq!(
            outward_exit(taken).run_v8(
                &Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", &[0; 4])])
            ),
            Observation::returned(&[Value::I32(if taken { 7 } else { 11 })]).with_memories(&[
                MemoryBytes::new("state", &[if taken { 0 } else { 3 }, 0, 0, 0])
            ])
        );
    }
    assert_eq!(
        unconditional_backedge().run_v8(
            &Input::call("run", &[Value::I32(3)])
                .with_memories(&[MemoryBytes::new("state", &[0; 4])])
        ),
        Observation::returned(&[Value::I32(21)])
            .with_memories(&[MemoryBytes::new("state", &[0; 4])])
    );
}
