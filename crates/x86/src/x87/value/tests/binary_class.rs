//! Native-format classes survive value operations without narrowing unknown inputs.

use super::*;
use wasmparser::{Operator, Parser, Payload};

#[test]
fn native_classes_control_expansion_and_exact_store_range() {
    for (format, sign, one_point_five, minimum, minimum_exponent, indefinite) in [
        (
            BinaryFormat::Binary32,
            0x8000_0000,
            0x3fc0_0000,
            0x0080_0000,
            0x3f81,
            0xffc0_0000,
        ),
        (
            BinaryFormat::Binary64,
            0x8000_0000_0000_0000,
            0x3ff8_0000_0000_0000,
            0x0010_0000_0000_0000,
            0x3c01,
            0xfff8_0000_0000_0000,
        ),
    ] {
        for right_known in [false, true] {
            let mut program = Program::new();
            let function = program
                .function(
                    Signature {
                        parameters: vec![Type::I64, Type::I64, Type::I1, Type::I1],
                        results: vec![Type::I64, Type::I16, Type::I64, Type::I1, Type::I16],
                    },
                    |body| {
                        let bits = body.parameter::<I64>(0)?;
                        let zero = bits.and(sign - 1).eq(0_u64);
                        let left = ExtendedValue::from_classified_binary(
                            format,
                            bits,
                            KnownClass::zero().select(&zero, &KnownClass::normal()),
                        );
                        let bits = body.parameter::<I64>(1)?;
                        let right = if right_known {
                            ExtendedValue::from_classified_binary(
                                format,
                                bits,
                                KnownClass::normal(),
                            )
                        } else {
                            // Normal after expansion does not exclude a narrow subnormal.
                            ExtendedValue::from_binary(format, bits).assume_normal()
                        };
                        let value = left
                            .select(&body.parameter::<I1>(2)?, &right)
                            .or_indefinite(&body.parameter::<I1>(3)?)
                            .change_sign(SignOperation::Negate);
                        let extended = value.bits();
                        let encoded = value.exact_conversion(format).unwrap();
                        body.return_((
                            extended.significand,
                            extended.sign_exponent,
                            encoded.bits,
                            encoded.tiny,
                            value.classification().tag(),
                        ))
                    },
                )
                .unwrap();
            program.export("views", function).unwrap();
            let bytes = program.compile().unwrap();
            let normalizes_subnormals =
                Parser::new(0)
                    .parse_all(&bytes)
                    .any(|payload| match payload.unwrap() {
                        Payload::CodeSectionEntry(body) => body
                            .get_operators_reader()
                            .unwrap()
                            .into_iter()
                            .any(|op| matches!(op.unwrap(), Operator::I64Clz)),
                        _ => false,
                    });
            assert_eq!(normalizes_subnormals, !right_known);
            let module = step::TestModule::new(&CompiledModule {
                bytes,
                entry: "views".into(),
                execution_target: None,
            });
            for (left, left_sig, left_exp) in [
                (0_u64, 0_u64, 0_u16),
                (sign, 0, 0x8000),
                (one_point_five, 0xc000_0000_0000_0000, 0x3fff),
                (minimum, 1 << 63, minimum_exponent),
            ] {
                let (right, right_exp) = if right_known {
                    (minimum, minimum_exponent)
                } else {
                    (
                        1,
                        if format == BinaryFormat::Binary32 {
                            0x3f6a
                        } else {
                            0x3bcd
                        },
                    )
                };
                for choose_left in [false, true] {
                    for invalid in [false, true] {
                        let (bits, sig, exp, tag, tiny) = if invalid {
                            (indefinite ^ sign, 0xc000_0000_0000_0000, 0x7fff, 2, false)
                        } else if choose_left {
                            (
                                left ^ sign,
                                left_sig,
                                left_exp ^ 0x8000,
                                u16::from(left_sig == 0),
                                false,
                            )
                        } else {
                            (right ^ sign, 1 << 63, right_exp ^ 0x8000, 0, !right_known)
                        };
                        let input = step::Input {
                            arguments: vec![
                                step::Argument::I64(left as i64),
                                step::Argument::I64(right as i64),
                                step::Argument::I32(i32::from(choose_left)),
                                step::Argument::I32(i32::from(invalid)),
                            ],
                            ..step::Input::new(&[])
                        };
                        assert_eq!(
                            module.observe(&input, 1).events,
                            vec![step::Event::Return {
                                outcome: step::Outcome::Returned(vec![
                                    step::Argument::I64(sig as i64),
                                    step::Argument::I32(i32::from(exp)),
                                    step::Argument::I64(bits as i64),
                                    step::Argument::I32(i32::from(tiny)),
                                    step::Argument::I32(i32::from(tag))
                                ]),
                                snapshot: step::Snapshot {
                                    cpu: vec![],
                                    guest: None
                                },
                            }]
                        );
                    }
                }
            }
        }
    }
}
