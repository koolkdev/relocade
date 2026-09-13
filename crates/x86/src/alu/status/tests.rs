use crate::test_step as step;

use super::StatusSource;
use crate::alu::ArithmeticOp;
use crate::flags::{Condition, StatusFlag};
use crate::CompiledModule;
use wasm86_compiler::{MemoryInt, Program, Signature, Type, I16, I32, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn auxiliary_carry<T: MemoryInt>() -> CompiledModule {
    let mut program = Program::new();
    for (name, operation) in [("add", ArithmeticOp::Add), ("sub", ArithmeticOp::Subtract)] {
        let function = program
            .function(
                Signature {
                    parameters: vec![T::TYPE; 2],
                    results: vec![Type::I1],
                },
                |body| {
                    // The addition can leave dirty upper carrier bits before the flag query.
                    let left = body.parameter::<T>(0)?.add(1);
                    let right = body.parameter::<T>(1)?;
                    let result = operation.result(&left, &right);
                    let source = StatusSource::Arithmetic {
                        operation,
                        left,
                        right,
                        result,
                    };
                    body.return_(source.flag(StatusFlag::AF))
                },
            )
            .unwrap();
        program.export(name, function).unwrap();
    }
    let logic = program
        .function(
            Signature {
                parameters: vec![T::TYPE; 2],
                results: vec![Type::I1],
            },
            |body| {
                let result = body.parameter::<T>(0)?.add(1).xor(body.parameter::<T>(1)?);
                body.return_(StatusSource::Logic { result }.flag(StatusFlag::AF))
            },
        )
        .unwrap();
    program.export("logic", logic).unwrap();
    CompiledModule {
        segment_profile: None,
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
                    results: vec![Type::I1],
                },
                |body| {
                    let left = body.parameter::<I32>(0)?;
                    let right = body.parameter::<I32>(1)?;
                    let operation = ArithmeticOp::Subtract;
                    let result = operation.result(&left, &right);
                    let source = StatusSource::Arithmetic {
                        operation,
                        left,
                        right,
                        result,
                    };
                    body.return_(source.condition(condition))
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

#[test]
fn auxiliary_carry_handles_nibble_boundaries_and_dirty_upper_bits() {
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
            ],
        ),
        (
            auxiliary_carry::<I16>(),
            vec![("add", 65534, 1, 1), ("sub", 65535, 1, 1)],
        ),
        (
            auxiliary_carry::<I32>(),
            vec![("add", -1, 1, 0), ("sub", -1, 1, 1)],
        ),
    ] {
        assert_auxiliary_carry(module, &cases);
    }
}

#[test]
fn logical_undefined_auxiliary_flag_uses_zero_policy() {
    for (module, cases) in [
        (
            auxiliary_carry::<I8>(),
            vec![
                ("logic", 14, 1, 0),
                ("logic", 255, 1, 0),
                ("logic", 255, 255, 0),
            ],
        ),
        (
            auxiliary_carry::<I16>(),
            vec![("logic", 65535, 1, 0), ("logic", 65535, 65535, 0)],
        ),
        (
            auxiliary_carry::<I32>(),
            vec![("logic", -1, 1, 0), ("logic", i32::MAX, -1, 0)],
        ),
    ] {
        assert_auxiliary_carry(module, &cases);
    }
}

#[test]
fn negation_auxiliary_carry_depends_on_the_original_low_nibble() {
    // The module adds one to its first parameter, giving a logical zero here.
    // These cases therefore query AF for zero minus the original operand.
    for (module, cases) in [
        (
            auxiliary_carry::<I8>(),
            vec![
                ("sub", 255, 0, 0),
                ("sub", 255, 15, 1),
                ("sub", 255, 16, 0),
                ("sub", 255, 128, 0),
                ("sub", 255, 255, 1),
            ],
        ),
        (
            auxiliary_carry::<I16>(),
            vec![
                ("sub", 65535, 0, 0),
                ("sub", 65535, 15, 1),
                ("sub", 65535, 16, 0),
                ("sub", 65535, 32768, 0),
                ("sub", 65535, 65535, 1),
            ],
        ),
        (
            auxiliary_carry::<I32>(),
            vec![
                ("sub", -1, 0, 0),
                ("sub", -1, 15, 1),
                ("sub", -1, 16, 0),
                ("sub", -1, i32::MIN, 0),
                ("sub", -1, -1, 1),
            ],
        ),
    ] {
        assert_auxiliary_carry(module, &cases);
    }
}

fn assert_auxiliary_carry(module: CompiledModule, cases: &[(&str, i32, i32, i32)]) {
    let mut module = step::TestModule::new(&module);
    for &(entry, left, right, expected) in cases {
        module.entry = entry.into();
        assert_return(&module, &[left, right], expected);
    }
}

fn assert_return(module: &step::TestModule, arguments: &[i32], expected: i32) {
    let input = step::Input {
        arguments: arguments.iter().copied().map(step::Argument::I32).collect(),
        ..step::Input::new(&[])
    };
    assert_eq!(
        module.observe(&input, 1),
        step::Observation {
            events: vec![step::Event::Return {
                outcome: step::Outcome::Returned(vec![step::Argument::I32(expected)]),
                snapshot: step::Snapshot {
                    cpu: vec![],
                    guest: None
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "{}({arguments:?})",
        module.entry,
    );
}
