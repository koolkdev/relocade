use crate::{
    body::{BlockItem, Edge, Exit, Layout, OperationKind},
    BuildError, FunctionKind, MemoryImport, Program, Signature, Type, I1, I32,
};

fn signature() -> Signature {
    Signature {
        parameters: vec![Type::I1],
        results: vec![Type::I32],
    }
}

#[test]
fn conditional_exits_keep_their_edge_scope_and_snapshot_producers() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let function = program.declare(signature());
    program
        .define(function, |mut body| {
            let choose = body.parameter::<I1>(0).unwrap();
            let result = body
                .block::<I32>(|mut block, exit| {
                    let before = block.load::<I32>(memory, 0)?;
                    block.branch_if(&choose, &exit, before)?;
                    let after = block.load::<I32>(memory, 4)?;
                    block.yield_if(after.eq(0), &after)?;
                    block.yield_(after)
                })
                .unwrap();
            body.return_(result)
        })
        .unwrap();
    let FunctionKind::Defined(Some(body)) = &program.functions[function.0].kind else {
        panic!("the function is complete");
    };
    let [Layout::Scope {
        body: layout,
        after,
        ..
    }, Layout::Block(_)] = body.layout.as_slice()
    else {
        panic!("the result belongs to the scope's continuation");
    };
    let [Layout::If {
        branch: first,
        taken: first_exit,
        ..
    }, Layout::If {
        branch: second,
        taken: second_exit,
        ..
    }, Layout::Block(_)] = layout.as_slice()
    else {
        panic!("two loads each precede a conditional exit");
    };
    let mut results = Vec::new();
    for (source, taken) in [(first, first_exit), (second, second_exit)] {
        let [BlockItem::Effect(effect)] = body.blocks[source.0].items.as_slice() else {
            panic!("the branch observes its own read snapshot");
        };
        let effect = &body.effects[effect.0];
        assert!(matches!(
            effect.operation.kind(),
            OperationKind::Load { .. }
        ));
        results.push(effect.results.clone());
        let [Layout::Block(edge)] = taken.as_slice() else {
            panic!("the taken arm is an edge");
        };
        assert_ne!(edge, source);
        assert!(body.blocks[edge.0].items.is_empty());
        assert!(
            matches!(&body.blocks[edge.0].exit, Exit::Jump(edge) if edge.target == *after && edge.arguments == effect.results)
        );
    }
    assert_ne!(results[0], results[1]);
    assert_eq!(
        body.blocks
            .iter()
            .flat_map(|block| block.exit.edges())
            .filter(|edge| edge.target == *after)
            .count(),
        3
    );
    assert!(program.compile().is_ok());
}

#[test]
fn conditional_exit_errors_leave_the_parent_open_and_do_not_attach_an_edge() {
    let mut foreign = Program::new();
    let foreign_function = foreign.declare(signature());
    foreign
        .define(foreign_function, |mut foreign_body| {
            let foreign_condition = foreign_body.parameter::<I1>(0).unwrap();
            let mut foreign_label = None;
            let foreign_result = foreign_body
                .block::<I32>(|block, exit| {
                    foreign_label = Some(exit);
                    block.yield_(0)
                })
                .unwrap();

            let mut program = Program::new();
            let memory = program.import_memory(MemoryImport {
                module: "test".into(),
                name: "memory".into(),
                minimum: 1,
                maximum: None,
                shared: false,
            });
            let function = program.declare(signature());
            program
                .define(function, |mut body| {
                    assert_eq!(body.yield_if(false, 7), Err(BuildError::InvalidYield));
                    let mut escaped_label = None;
                    let result = body
                        .block::<I32>(|mut block, exit| {
                            escaped_label = Some(exit.clone());
                            let mut child_value = None;
                            block.if_(true, |mut child| {
                                child_value = Some(child.load::<I32>(memory, 0)?);
                                Ok(())
                            })?;
                            let child_value = child_value.unwrap();
                            let before = (block.pending.current, block.pending.layout.len());
                            assert_eq!(
                                block.branch_if(false, &exit, ()),
                                Err(BuildError::ResultCount {
                                    expected: 1,
                                    actual: 0
                                })
                            );
                            assert_eq!(
                                block.branch_if(false, &exit, true),
                                Err(BuildError::TypeMismatch {
                                    expected: Type::I32,
                                    actual: Type::I1
                                })
                            );
                            assert_eq!(
                                block.yield_if(false, true),
                                Err(BuildError::TypeMismatch {
                                    expected: Type::I32,
                                    actual: Type::I1
                                })
                            );
                            assert_eq!(
                                block.branch_if(&foreign_condition, &exit, 7),
                                Err(BuildError::ForeignBody)
                            );
                            assert_eq!(
                                block.yield_if(&foreign_condition, 7),
                                Err(BuildError::ForeignBody)
                            );
                            assert_eq!(
                                block.branch_if(false, foreign_label.as_ref().unwrap(), 7),
                                Err(BuildError::ForeignBody)
                            );
                            assert_eq!(
                                block.branch_if(false, &exit, &child_value),
                                Err(BuildError::OutOfScope)
                            );
                            assert_eq!(
                                block.yield_if(false, &child_value),
                                Err(BuildError::OutOfScope)
                            );
                            assert_eq!(
                                block.branch_if(child_value.eq(0), &exit, 7),
                                Err(BuildError::OutOfScope)
                            );
                            assert_eq!((block.pending.current, block.pending.layout.len()), before);
                            block.if_(true, |mut arm| {
                                assert_eq!(arm.yield_if(false, 7), Err(BuildError::InvalidYield));
                                Ok(())
                            })?;
                            block.yield_if(false, 11)?;
                            block.yield_(7)
                        })
                        .unwrap();
                    assert_eq!(
                        body.branch_if(false, escaped_label.as_ref().unwrap(), 9),
                        Err(BuildError::OutOfScope)
                    );
                    body.return_(result)
                })
                .unwrap();
            assert!(program.compile().is_ok());
            foreign_body.return_(foreign_result)
        })
        .unwrap();
    assert!(foreign.compile().is_ok());
}

#[test]
fn a_general_if_keeps_its_control_boundary_when_the_parent_completes() {
    let mut program = Program::new();
    let function = program.declare(signature());
    program
        .define(function, |mut body| {
            let choose = body.parameter::<I1>(0).unwrap();
            body.if_(choose, |arm| arm.return_(11)).unwrap();
            body.return_(7)
        })
        .unwrap();
    let FunctionKind::Defined(Some(body)) = &program.functions[function.0].kind else {
        panic!("the function is complete");
    };
    let [Layout::If { join, .. }, Layout::Block(last)] = body.layout.as_slice() else {
        panic!("a conditional has a separate continuation");
    };
    assert_eq!(join, last);
    assert!(matches!(body.blocks[join.0].exit, Exit::Return(_)));
    assert!(program.compile().is_ok());
}

#[test]
fn a_nested_exit_keeps_the_conditional_label_it_targets() {
    let mut program = Program::new();
    let function = program.declare(signature());
    program
        .define(function, |mut body| {
            let choose = body.parameter::<I1>(0).unwrap();
            body.if_value::<()>(
                choose,
                |mut taken| {
                    let destination = taken.yield_target.as_ref().unwrap().target;
                    taken.block::<()>(|nested, _| {
                        // The nested block exits the surrounding conditional.
                        nested.terminate(|_| {
                            Ok(Exit::Jump(Edge {
                                target: destination,
                                arguments: vec![],
                            }))
                        })
                    })?;
                    taken.trap()
                },
                |_| Ok(()),
            )
            .unwrap();
            body.return_(7)
        })
        .unwrap();
    let FunctionKind::Defined(Some(body)) = &program.functions[function.0].kind else {
        panic!("the function is complete");
    };
    let [Layout::If { join, .. }, Layout::Block(last)] = body.layout.as_slice() else {
        panic!("a conditional has a separate continuation");
    };
    assert_eq!(join, last);
    assert!(matches!(body.blocks[join.0].exit, Exit::Return(_)));
    let bytes = program.compile().unwrap();
    wasmparser::Validator::new().validate_all(&bytes).unwrap();
}
