#[path = "bit_counts/logical.rs"]
mod logical;
#[path = "bit_counts/placement.rs"]
mod placement;

use crate::fixture::Fixture;
use wasm86_compiler::{IntType, Program, Signature, Type, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[test]
fn population_counts_use_logical_bits_and_native_carrier_operations() {
    fn population<T: IntType>(program: &mut Program, name: &str) {
        let function = program
            .function(
                Signature {
                    parameters: vec![T::TYPE],
                    results: vec![T::TYPE],
                },
                |body| {
                    let input = body.parameter::<T>(0)?;
                    body.return_(input.add(1).popcnt())
                },
            )
            .unwrap();
        program.export(name, function).unwrap();
    }
    let mut fixture = Fixture::new();
    population::<I1>(&mut fixture.program, "bits1");
    population::<I8>(&mut fixture.program, "bits8");
    population::<I16>(&mut fixture.program, "bits16");
    population::<I32>(&mut fixture.program, "bits32");
    population::<I64>(&mut fixture.program, "bits64");
    let xor = fixture
        .program
        .function(
            Signature {
                parameters: vec![Type::I32; 2],
                results: vec![Type::I32],
            },
            |body| {
                let left = body.parameter::<I32>(0)?;
                let right = body.parameter::<I32>(1)?;
                body.return_(left.xor(right))
            },
        )
        .unwrap();
    fixture.program.export("xor", xor).unwrap();
    let module = fixture.compile();
    let mut instance = module.instantiate();
    for (name, argument, expected) in [
        ("bits1", 0, 1),
        ("bits1", 1, 0),
        ("bits8", 254, 8),
        ("bits8", 255, 0),
        ("bits16", 65534, 16),
        ("bits16", 65535, 0),
        ("bits32", -2, 32),
        ("bits32", -1, 0),
    ] {
        assert_eq!(
            instance.call_export::<i32>(name, (argument,)).unwrap(),
            expected,
            "{name}({argument})"
        );
    }
    for (argument, expected) in [(-2_i64, 64), (-1, 0)] {
        assert_eq!(
            instance.call_export::<i64>("bits64", (argument,)).unwrap(),
            expected
        );
    }
    assert_eq!(
        instance.call_export::<i32>("xor", (-1, i32::MAX)).unwrap(),
        i32::MIN
    );
    let mut masks = 0;
    let mut counts32 = 0;
    let mut counts64 = 0;
    let mut xors = 0;
    for payload in Parser::new(0).parse_all(module.bytes()) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                match op.unwrap() {
                    Operator::I32And => masks += 1,
                    Operator::I32Popcnt => counts32 += 1,
                    Operator::I64Popcnt => counts64 += 1,
                    Operator::I32Xor => xors += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!((masks, counts32, counts64, xors), (3, 4, 1, 1));
}

#[test]
fn constant_xor_and_population_count_fold_without_runtime_work() {
    let module = Fixture::new().function(&[], &[Type::I64], |body| {
        let count = body
            .value::<I64>(0xff00_ff00_ff00_ff00u64)?
            .xor(0x0f0f_0f0f_0f0f_0f0fu64)
            .popcnt();
        body.return_(count)
    });
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i64>(()).unwrap(), 32);
    let bytes = module.bytes();
    Validator::new().validate_all(bytes).unwrap();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let operators = body
                .get_operators_reader()
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>();
            assert!(matches!(
                operators.as_slice(),
                [
                    Operator::I64Const { value: 32 },
                    Operator::Return,
                    Operator::End
                ]
            ));
        }
    }
}
