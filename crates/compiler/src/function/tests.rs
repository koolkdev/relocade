use crate::{BlockBuilder, BuildError, Program, Signature, Type, I1, I16, I32, I64, I8};
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
fn unwinding_a_definition_closes_its_values() {
    let mut program = Program::new();
    let retained = std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = retained.clone();
    let function = program.function::<fn() -> u32>(move |body, ()| {
        *capture.borrow_mut() = Some((body.value::<I32>(7)?, body.arena.clone()));
        panic!("construction interrupted");
    });
    program.export("run", function).unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| program.compile()));
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

#[test]
fn a_body_registers_a_helper_without_reopening_itself() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I1],
    };
    let outer = program.declare(signature.clone());
    program
        .define(outer, move |mut body| {
            assert_eq!(
                body.program().define(outer, move |body| body.trap()).err(),
                Some(BuildError::AlreadyDefined)
            );
            let input = body.parameter::<I32>(0).unwrap();
            let helper = body.program().raw_function(signature, move |helper| {
                let value = helper.parameter::<I32>(0)?;
                helper.return_(value.eq(0))
            });
            body.if_(input.eq(-1), |branch| branch.trap()).unwrap();
            let result = body.call::<I1>(helper, &[input.into()]).unwrap();
            body.return_(result)
        })
        .unwrap();
    program.export("run", outer).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut traps = 0;
    let mut calls = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                match op.unwrap() {
                    Operator::Unreachable => traps += 1,
                    Operator::Call { .. } => calls += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!((traps, calls), (1, 1));
}

/// Retains a value from a completed function for ownership-validation fixtures.
pub(crate) fn capture_from_body<T: 'static>(
    build: impl FnOnce(&mut BlockBuilder<'_>) -> T + 'static,
) -> T {
    let retained = std::rc::Rc::new(std::cell::RefCell::new(None));
    let capture = retained.clone();
    let mut program = Program::new();
    let function = program.function::<fn(u32)>(move |mut body, (_,)| {
        *capture.borrow_mut() = Some(build(&mut body));
        body.return_(())
    });
    program.export("capture", function).unwrap();
    program.compile().unwrap();
    let result = retained.borrow_mut().take().unwrap();
    result
}
