use crate::{BuildError, MemoryImport, Program, Signature, Type, I1, I32, I64};

fn signature() -> Signature {
    Signature {
        parameters: vec![],
        result: Some(Type::I32),
    }
}

#[test]
fn labels_are_confined_to_their_block_and_nested_descendants() {
    let mut program = Program::new();
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    let mut escaped = None;
    let value = body
        .block::<I32>(|mut block, label| {
            escaped = Some(label.clone());
            block.block::<()>(|mut nested, _| nested.if_(true, |inner| inner.branch(&label, 7)))?;
            block.yield_(11)
        })
        .unwrap();
    assert_eq!(
        body.if_(true, |branch| branch.branch(escaped.as_ref().unwrap(), 13)),
        Err(BuildError::OutOfScope)
    );
    assert_eq!(
        body.block::<I32>(|sibling, _| sibling.branch(escaped.as_ref().unwrap(), 17))
            .err(),
        Some(BuildError::OutOfScope)
    );
    body.return_(value).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn a_discarded_block_label_cannot_alias_a_later_block_at_the_same_site() {
    let mut program = Program::new();
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    let mut escaped = None;
    assert_eq!(
        body.block::<I32>(|_, label| {
            escaped = Some(label);
            Err(BuildError::UnknownParameter)
        })
        .err(),
        Some(BuildError::UnknownParameter)
    );
    assert_eq!(
        body.block::<I32>(|block, _| block.branch(escaped.as_ref().unwrap(), 7))
            .err(),
        Some(BuildError::OutOfScope)
    );
    let result = body
        .block::<I32>(|block, label| block.branch(&label, 11))
        .unwrap();
    body.return_(result).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn a_label_cannot_target_another_function_body() {
    let mut program = Program::new();
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    let result = body
        .block::<I32>(|mut block, label| {
            assert_eq!(
                block
                    .program()
                    .function(signature(), |other| other.branch(&label, 7))
                    .err(),
                Some(BuildError::ForeignBody)
            );
            block.yield_(11)
        })
        .unwrap();
    body.return_(result).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn branch_and_yield_arguments_validate_counts_logical_types_and_visibility() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
    });
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    assert_eq!(
        body.block::<(I32, I1)>(|block, label| block.branch(&label, 7))
            .err(),
        Some(BuildError::ArgumentCount {
            expected: 2,
            actual: 1
        })
    );
    assert_eq!(
        body.if_value::<(I32, I1)>(
            true,
            |arm| arm.yield_((7, false, 9)),
            |arm| arm.yield_((11, true))
        )
        .err(),
        Some(BuildError::ArgumentCount {
            expected: 2,
            actual: 3
        })
    );
    assert_eq!(
        body.block::<(I32, I1)>(|block, label| {
            let word = block.value::<I32>(1)?;
            block.branch(&label, (7, word))
        })
        .err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I1,
            actual: Type::I32
        })
    );
    let mut child_value = None;
    body.if_(true, |mut branch| {
        child_value = Some(branch.load::<I32>(memory, 0)?);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        body.block::<I32>(|block, label| block.branch(&label, child_value.as_ref().unwrap()))
            .err(),
        Some(BuildError::OutOfScope)
    );
    body.return_(17).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn nonempty_results_require_completed_arms_and_an_incoming_result() {
    let mut program = Program::new();
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    assert_eq!(
        body.block::<(I32, I64)>(|_, _| Ok(())).err(),
        Some(BuildError::IncompleteBranch)
    );
    assert_eq!(
        body.block::<I32>(|block, _| block.return_(7)).err(),
        Some(BuildError::MissingBranchValue)
    );
    assert_eq!(
        body.if_value::<(I32, I1)>(true, |arm| arm.return_(7), |arm| arm.trap())
            .err(),
        Some(BuildError::MissingBranchValue)
    );
    body.return_(17).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn a_swallowed_bad_outward_branch_cannot_become_implicit_fallthrough() {
    let mut program = Program::new();
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    assert_eq!(
        body.block::<()>(|mut block, label| {
            block.if_(true, |branch| {
                assert_eq!(
                    branch.branch(&label, 7),
                    Err(BuildError::ArgumentCount {
                        expected: 0,
                        actual: 1
                    })
                );
                Ok(())
            })
        }),
        Err(BuildError::IncompleteBranch)
    );
    body.return_(17).unwrap();
    assert!(program.compile().is_ok());
}

#[test]
fn failed_late_branches_discard_earlier_arms_and_their_imports() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
    });
    let function = program.declare(signature());
    let mut body = program.define(function).unwrap();
    assert_eq!(
        body.block::<(I32, I1)>(|mut block, label| {
            block.if_(true, |mut branch| {
                branch.store::<I32>(memory, 0, 9)?;
                branch.branch(&label, (7, true))
            })?;
            block.yield_(7)
        })
        .err(),
        Some(BuildError::ArgumentCount {
            expected: 2,
            actual: 1
        })
    );
    body.return_(17).unwrap();
    let bytes = program.compile().unwrap();
    assert!(wasmparser::Parser::new(0)
        .parse_all(&bytes)
        .all(|payload| !matches!(payload.unwrap(), wasmparser::Payload::ImportSection(_))));
}
