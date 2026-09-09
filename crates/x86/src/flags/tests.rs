use crate::test_step as step;

use super::{logic_flag, ArithmeticKind, ArithmeticSource, Condition, StatusFlag};
use crate::CompiledModule;
use wasm86_compiler::{MemoryInt, Program, Signature, Type, I16, I32, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn auxiliary_carry<T: MemoryInt>() -> CompiledModule {
    let mut program = Program::new();
    for (name, kind) in [("add", ArithmeticKind::Add), ("sub", ArithmeticKind::Sub)] {
        let function = program
            .function(
                Signature {
                    parameters: vec![T::TYPE; 2],
                    result: Type::I1,
                },
                |body| {
                    // The addition can leave dirty upper carrier bits before the flag query.
                    let left = body.parameter::<T>(0)?.add(1);
                    let right = body.parameter::<T>(1)?;
                    body.return_(ArithmeticSource::new(kind, left, right).flag(StatusFlag::AF))
                },
            )
            .unwrap();
        program.export(name, function).unwrap();
    }
    let logic = program
        .function(
            Signature {
                parameters: vec![T::TYPE; 2],
                result: Type::I1,
            },
            |body| {
                let result = body.parameter::<T>(0)?.add(1).xor(body.parameter::<T>(1)?);
                body.return_(logic_flag(&result, StatusFlag::AF))
            },
        )
        .unwrap();
    program.export("logic", logic).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "add".into(),
    }
}

#[test]
fn auxiliary_carry_is_a_nibble_test_without_a_full_flag_image() {
    let module = auxiliary_carry::<I8>();
    Validator::new().validate_all(&module.bytes).unwrap();
    let mut xors = 0;
    let mut population_counts = 0;
    for payload in Parser::new(0).parse_all(&module.bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operation in body.get_operators_reader().unwrap() {
                match operation.unwrap() {
                    Operator::I32Xor => xors += 1,
                    Operator::I32Popcnt => population_counts += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!((xors, population_counts), (4, 0));
}

#[test]
fn signed_cmp_conditions_use_original_operands_without_computing_flags() {
    let mut program = Program::new();
    for condition in [Condition::L, Condition::GE] {
        program
            .function(
                Signature {
                    parameters: vec![Type::I32; 2],
                    result: Type::I1,
                },
                |body| {
                    let left = body.parameter::<I32>(0)?;
                    let right = body.parameter::<I32>(1)?;
                    body.return_(ArithmeticSource::subtract(left, right).condition(condition))
                },
            )
            .unwrap();
    }
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut comparisons = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let operators = body
                .get_operators_reader()
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>();
            match operators.as_slice() {
                [Operator::LocalGet { local_index: 0 }, Operator::LocalGet { local_index: 1 }, comparison, Operator::Return, Operator::End] => {
                    comparisons.push(comparison.clone())
                }
                _ => panic!("direct comparison has unexpected flag work: {operators:?}"),
            }
        }
    }
    assert!(matches!(
        comparisons.as_slice(),
        [Operator::I32LtS, Operator::I32GeS]
    ));
}

fn check_auxiliary_carry(flags: &[&str]) {
    for (module, cases) in [
        (
            auxiliary_carry::<I8>(),
            vec![
                ("add", 14, 1, 1),
                ("add", 15, 1, 0),
                ("add", 254, 1, 1),
                ("add", 255, 1, 0),
                ("sub", 15, 1, 1),
                ("sub", 14, 1, 0),
                ("sub", 255, 1, 1),
                ("logic", 14, 1, 0),
                ("logic", 255, 1, 0),
            ],
        ),
        (
            auxiliary_carry::<I16>(),
            vec![
                ("add", 65534, 1, 1),
                ("sub", 65535, 1, 1),
                ("logic", 65535, 1, 0),
            ],
        ),
        (
            auxiliary_carry::<I32>(),
            vec![("add", -1, 1, 0), ("sub", -1, 1, 1), ("logic", -1, 1, 0)],
        ),
    ] {
        let mut module = step::ModuleFile::new(&module);
        for (entry, left, right, expected) in cases {
            module.entry = entry.into();
            let input = format!("[[],[],[],[[\"i32\",{left}],[\"i32\",{right}]]]");
            assert_eq!(
                module.observe(flags, &input, 1),
                format!("return {expected}\nstate \nguest unchanged\nmachine unchanged\n")
            );
        }
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn auxiliary_carry_executes_in_v8() {
    check_auxiliary_carry(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn auxiliary_carry_executes_in_v8_optimizing() {
    check_auxiliary_carry(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
