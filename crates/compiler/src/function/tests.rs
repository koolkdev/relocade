use crate::{BlockBuilder, BuildError, Program, Type, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[test]
fn constant_queries_report_construction_folds_without_changing_code() {
    fn compile(query: bool) -> Vec<u8> {
        let mut program = Program::new();
        let function = program.function::<fn(u32) -> u32>(move |mut body, (input,)| {
            if query {
                assert_eq!(body.constant_bits::<I1>(3)?, Some(1));
                assert_eq!(body.constant_bits::<I8>(0x1ff)?, Some(255));
                assert_eq!(body.constant_bits::<I16>(-1)?, Some(65535));
                assert_eq!(body.constant_bits::<I32>(-1)?, Some(0xffff_ffff));
                assert_eq!(body.constant_bits::<I64>(u64::MAX)?, Some(u64::MAX));
                let narrow = body.value::<I32>(0x1ff)?.truncate::<I8>();
                assert_eq!(narrow.constant_bits()?, Some(255));
                assert_eq!(body.constant_bits(narrow)?, Some(255));
                assert_eq!(input.mul(0).add(17).constant_bits()?, Some(17));
                assert_eq!(body.constant_bits(input.mul(0).add(17))?, Some(17));
                assert_eq!(body.constant_bits(&input)?, None);
                assert_eq!(body.constant_bits(input.add(1))?, None);
            }
            body.if_(input.eq(17), |branch| {
                if query {
                    // The guard becomes a constant fact only during placement.
                    assert_eq!(branch.constant_bits(&input)?, None);
                    assert_eq!(input.constant_bits()?, None);
                }
                branch.return_(&input)
            })?;
            body.return_(input.add(1))
        });
        program.export("run", function).unwrap();
        program.compile().unwrap()
    }

    let without_queries = compile(false);
    let with_queries = compile(true);
    Validator::new().validate_all(&with_queries).unwrap();
    assert_eq!(with_queries, without_queries);
}

#[test]
fn known_bits_follow_guards_without_changing_values() {
    let mut program = Program::new();
    let function = program.function::<fn(u8, u8, u32) -> u32>(|mut body, (pc, rc, input)| {
        let mode = pc.and(3).eq(2).and(rc.and(3).eq(0));
        assert_eq!(body.known_bits(&mode)?, None);
        body.if_(mode.eq(false), |arm| arm.return_(0))?;
        assert_eq!(body.known_bits(pc.and(3))?, Some(2));
        assert_eq!(body.known_bits(rc.and(3))?, Some(0));
        assert_eq!(body.known_bits(&pc)?, None); // The unused high bits remain unknown.
        assert_eq!(body.constant_bits(&mode)?, None);
        assert_eq!(mode.constant_bits()?, None);

        body.if_else(
            input.eq(17),
            |arm| {
                assert_eq!(arm.known_bits(&input)?, Some(17));
                assert_eq!(arm.known_bits(&mode)?, Some(1));
                Ok(())
            },
            |arm| {
                assert_eq!(arm.known_bits(input.eq(17))?, Some(0));
                Ok(())
            },
        )?;
        assert_eq!(body.known_bits(&input)?, None);
        assert_eq!(
            body.if_(input.eq(17), |arm| {
                assert_eq!(arm.known_bits(&input)?, Some(17));
                Err(BuildError::UnknownParameter)
            }),
            Err(BuildError::UnknownParameter)
        );
        assert_eq!(body.known_bits(&input)?, None);

        body.if_value::<()>(input.eq(17), |arm| arm.yield_(()), |_| Ok(()))?;
        assert_eq!(body.known_bits(&input)?, None); // A yield also reaches the join.
        body.loop_::<I32, ()>(17, |mut iteration, labels, current| {
            assert_eq!(iteration.known_bits(&current)?, None);
            assert_eq!(iteration.known_bits(&mode)?, Some(1));
            iteration.branch_if(current.eq(19), &labels.exit, ())?;
            assert_eq!(iteration.known_bits(current.eq(19))?, Some(0));
            iteration.branch(&labels.again, current.add(1))
        })?;
        assert_eq!(body.known_bits(&input)?, None);
        assert_eq!(body.known_bits(&mode)?, Some(1));
        body.return_(input)
    });
    program.export("run", function).unwrap();
    Validator::new()
        .validate_all(&program.compile().unwrap())
        .unwrap();
}

#[test]
fn known_bits_belong_to_the_guarded_snapshot() {
    fn compile(query: bool) -> Vec<u8> {
        let mut program = Program::new();
        let memory = program.import_memory(crate::MemoryImport {
            module: "test".into(),
            name: "memory".into(),
            minimum: 1,
            maximum: None,
            shared: false,
        });
        let function = program.function::<fn() -> u32>(move |mut body, ()| {
            let before = body.load::<I32>(memory, 0)?;
            body.if_(before.ne(7), |arm| arm.return_(0))?;
            body.store::<I32>(memory, 0, 42)?;
            let after = body.load::<I32>(memory, 0)?;
            if query {
                assert_eq!(body.known_bits(&before)?, Some(7));
                assert_eq!(body.known_bits(&after)?, None);
            }
            body.return_(after)
        });
        program.export("run", function).unwrap();
        program.compile().unwrap()
    }
    let bytes = compile(true);
    Validator::new().validate_all(&bytes).unwrap();
    assert_eq!(bytes, compile(false));
}

#[test]
fn unwinding_a_definition_closes_its_values() {
    let mut program = Program::new();
    let retained = std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = retained.clone();
    let function = program.declare(crate::Signature {
        parameters: vec![],
        results: vec![Type::I32],
    });
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        build_body(program, function, move |body| {
            *capture.borrow_mut() = Some((body.value::<I32>(7)?, body.arena.clone()));
            panic!("construction interrupted");
        })
    }));
    assert!(panic.is_err());
    let (value, arena) = retained.borrow_mut().take().unwrap();
    assert_eq!(
        value.checked_expression(&arena, 0),
        Err(BuildError::BodyClosed)
    );
}

#[test]
fn an_ignored_return_error_fails_compilation() {
    let mut program = Program::new();
    let function = program.function::<fn() -> u32>(move |body, ()| {
        assert!(body.return_(true).is_err());
        Ok(())
    });
    program.export("run", function).unwrap();
    assert_eq!(
        program.compile(),
        Err(BuildError::TypeMismatch {
            expected: Type::I32,
            actual: Type::I1,
        })
    );
}

#[test]
fn completed_functions_can_be_called_and_exported_with_logical_signatures() {
    let mut program = Program::new();
    let is_zero =
        program.function::<fn(u32) -> bool>(move |body, (value,)| body.return_(value.eq(0)));
    let run = program.function::<fn(u32) -> bool>(move |mut body, (input,)| {
        let result = body.call::<I1>(is_zero, &[input.into()])?;
        body.return_(result)
    });
    program.export("is_zero", is_zero).unwrap();
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let calls = Parser::new(0)
        .parse_all(&bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(
                body.get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .filter(|operator| matches!(operator, Ok(Operator::Call { .. })))
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(calls, 1);
}

#[test]
fn a_callback_error_fails_compilation_even_after_completing_its_body() {
    let mut program = Program::new();
    let function = program.function::<fn() -> u32>(move |body, ()| {
        body.return_(7)?;
        Err(BuildError::BodyClosed)
    });
    program.export("run", function).unwrap();
    assert_eq!(program.compile(), Err(BuildError::BodyClosed));
}

/// Runs a body locally so ownership fixtures can inspect its closed values.
pub(crate) fn build_body(
    program: Program,
    function: crate::Func,
    build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
) -> Result<(), BuildError> {
    super::build_function(&program.declarations, function, build).map(|_| ())
}

/// Retains a value from a completed function for ownership-validation fixtures.
pub(crate) fn capture_from_body<T>(build: impl FnOnce(&mut BlockBuilder<'_>) -> T) -> T {
    let mut program = Program::new();
    let function = program.declare(crate::Signature {
        parameters: vec![Type::I32],
        results: vec![],
    });
    let mut retained = None;
    build_body(program, function, |mut body| {
        retained = Some(build(&mut body));
        body.return_(())
    })
    .unwrap();
    retained.unwrap()
}
