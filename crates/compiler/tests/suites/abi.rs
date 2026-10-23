use crate::wasm::TestModule;
use relocade_compiler::{abi::Function, BuildError, Program, Signature, Type, I1, I16, I32, I64};

#[path = "abi/derived.rs"]
mod derived;

#[test]
fn rust_declarations_keep_logical_widths_and_nested_shapes() {
    type Query = fn(bool, i8, u16, (u32, [u64; 2])) -> (bool, [i16; 2], f64);
    const QUERY: Function<Query> = Function::new("example", "query");
    assert_eq!(
        QUERY.signature(),
        Signature {
            parameters: vec![
                Type::I1,
                Type::I8,
                Type::I16,
                Type::I32,
                Type::I64,
                Type::I64
            ],
            results: vec![Type::I1, Type::I16, Type::I16, Type::F64],
        }
    );
}

#[test]
fn typed_bodies_and_calls_project_parameters_and_results() {
    type Calculate = fn((bool, [u16; 2]), u32) -> (u16, u32);
    const CALCULATE: Function<Calculate> = Function::new("example", "calculate");
    const RUN: Function<fn() -> u64> = Function::new("example", "run");
    let mut program = Program::new();
    let calculate = CALCULATE
        .define(&mut program, |body, ((bit, [a, b]), word)| {
            body.return_((a.add(b), word.add(bit.unsigned().extend::<I32>())))
        })
        .unwrap();
    RUN.define(&mut program, |mut body, ()| {
        let (sum, word) = calculate.call(&mut body, ((true, [1, 65535]), 41))?;
        body.return_(
            sum.unsigned()
                .extend::<I64>()
                .or(word.unsigned().extend::<I64>().shl(32)),
        )
    })
    .unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    assert_eq!(module.instantiate().call::<i64>(()).unwrap(), 42_i64 << 32);
}

#[test]
fn failed_typed_definition_releases_its_export_name() {
    const RUN: Function<fn() -> bool> = Function::new("example", "run");
    let mut program = Program::new();
    assert!(matches!(
        RUN.define(&mut program, |_, ()| Ok(())),
        Err(BuildError::MissingBody)
    ));
    RUN.define(&mut program, |body, ()| body.return_(true))
        .unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    assert_eq!(module.instantiate().call::<i32>(()).unwrap(), 1);
}

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
