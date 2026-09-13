use crate::{
    control::{Site, Target},
    BuildError, FunctionKind, MemoryImport, Operation, Program, Signature, Terminal, Type,
    ValueKind, I1, I32,
};

fn signature() -> Signature {
    Signature {
        parameters: vec![Type::I1],
        results: vec![Type::I32],
    }
}

#[test]
fn conditional_exits_keep_their_edge_scope_and_adjacent_authored_sites() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
    });
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    let choose = body.parameter::<I1>(0).unwrap();
    let result = body
        .block::<I32>(|mut block, exit| {
            let before = block.load::<I32>(memory, 0)?;
            block.branch_if(&choose, &exit, before)?;
            let after = block.load::<I32>(memory, 4)?;
            block.yield_if(choose, &after)?;
            block.yield_(after)
        })
        .unwrap();
    body.return_(result).unwrap();
    let FunctionKind::Defined(Some(body)) = &program.functions[function.0].kind else {
        panic!("the function is complete");
    };
    let [Operation::Block { region, .. }] = body.region.operations.as_slice() else {
        panic!("the result belongs to the block");
    };
    assert_eq!(region.operations.len(), 4);
    let mut edge_scopes = Vec::new();
    for (index, operation) in region.operations.iter().enumerate() {
        match operation {
            Operation::Load(value) => {
                let ValueKind::Load { site, .. } = body.values[*value].kind else {
                    panic!("a load operation names its load value");
                };
                assert!(
                    site == Site {
                        region: region.id,
                        index
                    }
                );
            }
            Operation::BranchIf { taken, .. } => {
                assert_ne!(taken.id, region.id);
                assert!(taken.operations.is_empty());
                assert!(
                    matches!(&taken.terminal, Some(Terminal::Branch { target, arguments })
                    if *target == Target::exit(Site { region: body.region.id, index: 0 })
                        && arguments.len() == 1)
                );
                edge_scopes.push(taken.id);
            }
            _ => panic!("the authored sequence contains loads and conditional exits"),
        }
    }
    assert_eq!(edge_scopes.len(), 2);
    assert_ne!(edge_scopes[0], edge_scopes[1]);
    let children: Vec<_> = region
        .operations
        .iter()
        .enumerate()
        .flat_map(|(index, operation)| operation.children().map(move |child| (index, child.id)))
        .collect();
    assert_eq!(children, vec![(1, edge_scopes[0]), (3, edge_scopes[1])]);
    assert_eq!(region.walk().count(), 3);
    assert_eq!(
        region
            .exits_to(Target::exit(Site {
                region: body.region.id,
                index: 0
            }))
            .count(),
        3
    );
    assert!(matches!(region.terminal, Some(Terminal::Branch { .. })));
    assert!(program.compile().is_ok());
}

#[test]
fn conditional_exit_errors_leave_the_parent_open_and_do_not_attach_an_edge() {
    let mut foreign = Program::new();
    let foreign_function = foreign.declare(signature());
    let mut foreign_body = foreign.define(foreign_function).unwrap();
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
    });
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    assert_eq!(body.yield_if(false, 7), Err(BuildError::InvalidYield));
    let mut escaped_label = None;
    let result = body
        .block::<I32>(|mut block, exit| {
            escaped_label = Some(exit.clone());
            let mut child_value = None;
            block.if_(false, |mut child| {
                child_value = Some(child.load::<I32>(memory, 0)?);
                Ok(())
            })?;
            let child_value = child_value.unwrap();
            let before = block.region.operations.len();
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
            assert_eq!(block.region.operations.len(), before);
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
    body.return_(result).unwrap();
    foreign_body.return_(foreign_result).unwrap();
    assert!(program.compile().is_ok());
    assert!(foreign.compile().is_ok());
}

#[test]
fn a_general_if_keeps_its_control_boundary_when_the_parent_completes() {
    let mut program = Program::new();
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    let choose = body.parameter::<I1>(0).unwrap();
    body.if_(choose, |arm| arm.return_(11)).unwrap();
    body.return_(7).unwrap();
    let FunctionKind::Defined(Some(body)) = &program.functions[function.0].kind else {
        panic!("the function is complete");
    };
    assert!(matches!(
        body.region.operations.as_slice(),
        [Operation::If { .. }]
    ));
    assert!(matches!(body.region.terminal, Some(Terminal::Return(_))));
    assert!(program.compile().is_ok());
}

#[test]
fn a_nested_exit_keeps_the_conditional_label_it_targets() {
    let mut program = Program::new();
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    let choose = body.parameter::<I1>(0).unwrap();
    let target = Target::exit(body.site());
    body.if_(choose, |mut taken| {
        taken.block::<()>(|nested, _| {
            // This valid internal edge exits the If itself, a label the public
            // no-result builder does not expose.
            nested.complete(Terminal::Branch {
                target,
                arguments: vec![],
            })
        })?;
        taken.trap()
    })
    .unwrap();
    body.return_(7).unwrap();
    let FunctionKind::Defined(Some(body)) = &program.functions[function.0].kind else {
        panic!("the function is complete");
    };
    assert!(matches!(
        body.region.operations.as_slice(),
        [Operation::If { .. }]
    ));
    assert!(matches!(body.region.terminal, Some(Terminal::Return(_))));
    let bytes = program.compile().unwrap();
    wasmparser::Validator::new().validate_all(&bytes).unwrap();
}
