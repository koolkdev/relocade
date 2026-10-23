use super::*;
use crate::floating::BinaryFormat;
use crate::{test_step as step, CompiledModule};
use relocade_compiler::{Program, I32, I64};

#[test]
fn loaded_binary64_values_compare_without_source_exceptions() {
    let mut program = Program::new();
    let function = program
        .function::<fn(u64) -> (u32, u64)>(|body, _| {
            let source = BinaryFormat::Binary64.decode(&body.parameter::<I64>(0)?);
            let value = source.loaded_value();
            let minimum_normal = BinaryFormat::Binary64
                .decode(&0x0010_0000_0000_0000_u64.into())
                .loaded_value();
            let compared =
                BinaryOperands::new(&value, &minimum_normal).compare(ComparisonKind::Unordered);
            // Ordering predicates have no contract for unordered operands.
            let ordered = compared.unordered.eq(false);
            let comparison = [
                ordered.and(compared.less),
                ordered.and(compared.equal),
                compared.unordered,
                compared.invalid,
                compared.denormal,
            ]
            .into_iter()
            .enumerate()
            .fold(Val::<I32>::from(0), |bits, (index, flag)| {
                bits.or(flag.unsigned().extend::<I32>().shl(index as u32))
            });
            body.return_((comparison, value.binary64_value().unwrap().to_bits()))
        })
        .unwrap();
    program.export("classify", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "classify".into(),
        execution_target: None,
    });
    // Loaded subnormals are normal extended values; loaded SNaNs are quiet.
    // Comparison bits are LT/EQ/UN/IE/DE.
    for (bits, comparison, loaded_bits) in [
        (1_u64, 1, 1_u64),
        (0x000f_ffff_ffff_ffff, 1, 0x000f_ffff_ffff_ffff),
        (0x0010_0000_0000_0000, 2, 0x0010_0000_0000_0000),
        (0, 1, 0),
        (0x8000_0000_0000_0000, 1, 0x8000_0000_0000_0000),
        (0x7ff0_0000_0000_0000, 0, 0x7ff0_0000_0000_0000),
        (0x7ff8_0000_0000_0001, 4, 0x7ff8_0000_0000_0001),
        (0x7ff0_0000_0000_0001, 4, 0x7ff8_0000_0000_0001),
    ] {
        let input = step::Input {
            arguments: vec![step::Argument::I64(bits as i64)],
            ..step::Input::new(&[])
        };
        let expected = vec![step::Event::Return {
            outcome: step::Outcome::Returned(vec![
                step::Argument::I32(comparison),
                step::Argument::I64(loaded_bits as i64),
            ]),
            snapshot: step::Snapshot {
                cpu: vec![],
                guest: None,
            },
        }];
        assert_eq!(module.observe(&input, 1).events, expected);
    }
}

#[test]
fn binary32_promotion_preserves_ordering_and_source_exceptions() {
    for memory in [false, true] {
        let mut program = Program::new();
        let function = program
            .function::<fn(u64, u32) -> u32>(|body, _| {
                let left = BinaryFormat::Binary64
                    .decode(&body.parameter::<I64>(0)?)
                    .loaded_value();
                let right = BinaryFormat::Binary32
                    .decode(&body.parameter::<I32>(1)?.unsigned().extend::<I64>());
                let operands = if memory {
                    BinaryOperands::from_binary(&left, &right)
                } else {
                    BinaryOperands::new(&left, &right.loaded_value())
                };
                let compared = operands.compare(ComparisonKind::Unordered);
                let ordered = compared.unordered.eq(false);
                let bits = [
                    ordered.and(compared.less),
                    ordered.and(compared.equal),
                    compared.unordered,
                    compared.invalid,
                    compared.denormal,
                ]
                .into_iter()
                .enumerate()
                .fold(Val::<I32>::from(0), |bits, (index, flag)| {
                    bits.or(flag.unsigned().extend::<I32>().shl(index as u32))
                });
                body.return_(bits)
            })
            .unwrap();
        program.export("compare", function).unwrap();
        let module = step::TestModule::new(&CompiledModule {
            bytes: program.compile().unwrap(),
            entry: "compare".into(),
            execution_target: None,
        });
        // LT/EQ/UN/IE/DE: loading quiets SNaN and normalizes narrow denormals;
        // a memory operand must still report its original exception evidence.
        for (left, right, loaded, direct) in [
            (0x3ff0_0000_0000_0001_u64, 0x3f80_0000_u32, 0, 0),
            (0x3ff0_0000_0000_0001, 0x3f80_0001, 1, 1),
            (0x8000_0000_0000_0000, 0, 2, 2),
            (0x36a0_0000_0000_0000, 1, 2, 18),
            (0, 0x8000_0001, 0, 16),
            (0x7ff0_0000_0000_0000, 0x7f80_0000, 2, 2),
            (0xfff0_0000_0000_0000, 0xff7f_ffff, 1, 1),
            (0x3ff0_0000_0000_0000, 0x7fc0_0042, 4, 4),
            (0x3ff0_0000_0000_0000, 0x7f80_0042, 4, 12),
        ] {
            let input = step::Input {
                arguments: vec![
                    step::Argument::I64(left as i64),
                    step::Argument::I32(right as i32),
                ],
                ..step::Input::new(&[])
            };
            assert_eq!(
                module.observe(&input, 1).events,
                vec![step::Event::Return {
                    outcome: step::Outcome::Returned(vec![step::Argument::I32(if memory {
                        direct
                    } else {
                        loaded
                    })]),
                    snapshot: step::Snapshot {
                        cpu: vec![],
                        guest: None
                    }
                }]
            );
        }
    }
}

#[test]
fn proven_exact_inputs_emit_native_ordering() {
    use wasmparser::{Operator, Parser, Payload};
    for integer in [false, true] {
        let mut program = Program::new();
        let function = program
            .function::<fn(u32, u32) -> (bool, bool)>(|body, _| {
                let left = BinaryFormat::Binary32
                    .decode(&body.parameter::<I32>(0)?.unsigned().extend::<I64>())
                    .loaded_value();
                let input = body.parameter::<I32>(1)?;
                let right = if integer {
                    crate::x87::ExtendedValue::from_signed_integer(&input)
                } else {
                    BinaryFormat::Binary32
                        .decode(&input.unsigned().extend::<I64>())
                        .loaded_value()
                };
                let comparison =
                    BinaryOperands::new(&left, &right).compare(ComparisonKind::Unordered);
                body.return_((comparison.less, comparison.equal))
            })
            .unwrap();
        program.export("compare", function).unwrap();
        let bytes = program.compile().unwrap();
        let ops: Vec<_> = Parser::new(0)
            .parse_all(&bytes)
            .flat_map(|payload| match payload.unwrap() {
                Payload::CodeSectionEntry(body) => body
                    .get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .map(Result::unwrap)
                    .collect(),
                _ => vec![],
            })
            .collect();
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, Operator::F64Lt))
                .count(),
            1
        );
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, Operator::F64Eq))
                .count(),
            1
        );
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, Operator::F64PromoteF32))
                .count(),
            if integer { 1 } else { 2 }
        );
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, Operator::F64ConvertI32S))
                .count(),
            usize::from(integer)
        );
        assert!(!ops
            .iter()
            .any(|op| matches!(op, Operator::I64Clz | Operator::I64LtU | Operator::I64Eq)));
    }
}
