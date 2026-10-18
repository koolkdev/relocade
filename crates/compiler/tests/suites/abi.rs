use wasm86_compiler::{abi::Function, BuildError, Program, Signature, Type, I1, I16, I32, I64};

#[test]
fn declarations_keep_logical_widths_and_multiple_results() {
    type Query = fn(I16, I32) -> (I1, [I32; 2], I64);
    const QUERY: Function<Query> = Function::new("example", "query");
    assert_eq!(
        QUERY.signature(),
        Signature {
            parameters: vec![Type::I16, Type::I32],
            results: vec![Type::I1, Type::I32, Type::I32, Type::I64],
        }
    );
}

#[test]
fn exports_check_logical_types_before_registering_the_name() {
    const NARROW: Function<fn(I16) -> I16> = Function::new("example", "identity");
    const WIDE: Function<fn(I32) -> I32> = Function::new("example", "identity");
    let mut program = Program::new();
    let function = program
        .function(WIDE.signature(), |body| {
            let value = body.parameter::<I32>(0)?;
            body.return_(value)
        })
        .unwrap();
    assert_eq!(
        NARROW.export(&mut program, function),
        Err(BuildError::TypeMismatch {
            expected: Type::I16,
            actual: Type::I32
        })
    );
    WIDE.export(&mut program, function).unwrap();
    program.compile().unwrap();
}

#[test]
fn exports_reject_parameter_and_result_count_mismatches() {
    const READ: Function<fn(I32) -> I64> = Function::new("example", "read");
    let mut program = Program::new();
    let no_parameters = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I64],
    });
    assert_eq!(
        READ.export(&mut program, no_parameters),
        Err(BuildError::ArgumentCount {
            expected: 1,
            actual: 0
        })
    );
    let no_result = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![],
    });
    assert_eq!(
        READ.export(&mut program, no_result),
        Err(BuildError::ResultCount {
            expected: 1,
            actual: 0
        })
    );
}
