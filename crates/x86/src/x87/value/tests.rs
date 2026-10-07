use super::*;
use crate::{test_step as step, CompiledModule};
use wasm86_compiler::{Program, Signature, Type};
use wasmparser::{Operator, Parser, Payload};

fn non_normal_classes(value: &ExtendedValue) -> [Val<I1>; 7] {
    [
        value.zero(),
        value.denormal(),
        value.special_exponent(),
        value.infinity(),
        value.nan(),
        value.signaling_nan(),
        value.unsupported(),
    ]
}

fn class_flags(value: &ExtendedValue) -> Val<I32> {
    non_normal_classes(value)
        .into_iter()
        .enumerate()
        .fold(Val::<I32>::from(0_u32), |bits, (index, class)| {
            bits.or(class.unsigned().extend::<I32>().shl(index as u32))
        })
}

#[test]
fn known_normal_and_zero_values_reuse_classification_after_selection() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I16, Type::I1, Type::I1],
                results: vec![Type::I16, Type::I1, Type::I32],
            },
            |body| {
                let normal = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(0)?,
                    sign_exponent: body.parameter::<I16>(1)?,
                })
                .assume_class(Classification::normal());
                let negative = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(0)?,
                    sign_exponent: body.parameter::<I16>(1)?.xor(0x8000),
                })
                .assume_class(Classification::normal());
                let zero = ExtendedValue::from_bits(ExtendedBits {
                    significand: 0_u64.into(),
                    sign_exponent: 0x8000.into(),
                })
                .assume_class(Classification::zero());
                let value = normal
                    .select(&body.parameter::<I1>(2)?, &negative)
                    .select(&body.parameter::<I1>(3)?, &zero);
                body.return_((value.tag(), value.normal(), class_flags(&value)))
            },
        )
        .unwrap();
    program.export("classify", function).unwrap();
    let bytes = program.compile().unwrap();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operator in body.get_operators_reader().unwrap() {
                assert!(
                    !matches!(operator.unwrap(), Operator::LocalGet { local_index: 0 | 1 }),
                    "classification must not read the known value's bits"
                );
            }
        }
    }
}

#[test]
fn known_classes_survive_selection_and_indefinite_replacement() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I16, Type::I1, Type::I1, Type::I1],
                results: vec![
                    Type::I16,
                    Type::I1,
                    Type::I32,
                    Type::I16,
                    Type::I1,
                    Type::I32,
                ],
            },
            |body| {
                let normal = ExtendedValue::from_bits(ExtendedBits {
                    significand: (1_u64 << 63).into(),
                    sign_exponent: 0x3fff.into(),
                })
                .assume_class(Classification::normal());
                let zero = ExtendedValue::from_bits(ExtendedBits {
                    significand: 0_u64.into(),
                    sign_exponent: 0x8000.into(),
                })
                .assume_class(Classification::zero());
                let candidate = zero
                    .select(&body.parameter::<I1>(2)?, &normal)
                    .or_indefinite(&body.parameter::<I1>(3)?);
                let unknown = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(0)?,
                    sign_exponent: body.parameter::<I16>(1)?,
                });
                let value = candidate.select(&body.parameter::<I1>(4)?, &unknown);
                let known = candidate.select(&body.parameter::<I1>(4)?, &normal);
                body.return_((
                    value.tag(),
                    value.normal(),
                    class_flags(&value),
                    known.tag(),
                    known.normal(),
                    class_flags(&known),
                ))
            },
        )
        .unwrap();
    program.export("classify", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "classify".into(),
        execution_profile: None,
    });
    // Predicate bits: zero, denormal, special exponent, infinity, NaN, SNaN,
    // unsupported. Expectations come directly from these extended encodings.
    for (significand, exponent, tag, classes) in [
        (1_u64 << 63, 1, 0, 0),
        (0, 0x8000, 1, 0b0000001),
        (1, 0, 2, 0b0000010),
        (1_u64 << 63, 0, 2, 0b0000010),
        (1_u64 << 63, 0x7fff, 2, 0b0001100),
        (0xc000_0000_0000_0042, 0xffff, 2, 0b0010100),
        (0x8000_0000_0000_0001, 0x7fff, 2, 0b0110100),
        (1, 0x3fff, 2, 0b1000000),
    ] {
        for zero in [false, true] {
            for invalid in [false, true] {
                for use_candidate in [false, true] {
                    let (tag, classes) = match (use_candidate, invalid) {
                        (true, false) => {
                            if zero {
                                (1, 1)
                            } else {
                                (0, 0)
                            }
                        }
                        (true, true) => (2, 0b0010100),
                        (false, _) => (tag, classes),
                    };
                    let input = step::Input {
                        arguments: vec![
                            step::Argument::I64(significand as i64),
                            step::Argument::I32(exponent),
                            step::Argument::I32(i32::from(zero)),
                            step::Argument::I32(i32::from(invalid)),
                            step::Argument::I32(i32::from(use_candidate)),
                        ],
                        ..step::Input::new(&[])
                    };
                    let (known_tag, known_classes) = match (use_candidate, invalid, zero) {
                        (true, true, _) => (2, 0b0010100),
                        (true, false, true) => (1, 1),
                        _ => (0, 0),
                    };
                    let expected = vec![step::Event::Return {
                        outcome: step::Outcome::Returned(vec![
                            step::Argument::I32(tag),
                            step::Argument::I32(i32::from(tag == 0)),
                            step::Argument::I32(classes),
                            step::Argument::I32(known_tag),
                            step::Argument::I32(i32::from(known_tag == 0)),
                            step::Argument::I32(known_classes),
                        ]),
                        snapshot: step::Snapshot {
                            cpu: vec![],
                            guest: None,
                        },
                    }];
                    assert_eq!(module.observe(&input, 1).events, expected);
                }
            }
        }
    }
}

#[test]
fn precision53_views_agree_after_selection_and_indefinite_replacement() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I16, Type::I1, Type::I1],
                results: vec![Type::I64, Type::I16, Type::I64],
            },
            |body| {
                let integer = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(0)?,
                    sign_exponent: body.parameter::<I16>(1)?,
                })
                .assume_precision53();
                let native = ExtendedValue::from_precision53(Precision53::from_significand(
                    1.5.into(),
                    0x4000.into(),
                ));
                let value = integer
                    .select(&body.parameter::<I1>(2)?, &native)
                    .or_indefinite(&body.parameter::<I1>(3)?);
                let bits = value.bits();
                body.return_((
                    bits.significand,
                    bits.sign_exponent,
                    value.precision53_significand().unwrap().to_bits(),
                ))
            },
        )
        .unwrap();
    program.export("views", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "views".into(),
        execution_profile: None,
    });
    for (significand, exponent, coefficient) in [
        (0_u64, 0x8000, 0_u64),
        (0xa000_0000_0000_0000, 1, 0x3ff4_0000_0000_0000),
        (0xffff_ffff_ffff_f800, 0xfffe, 0x3fff_ffff_ffff_ffff),
    ] {
        for integer in [false, true] {
            for invalid in [false, true] {
                let input = step::Input {
                    arguments: vec![
                        step::Argument::I64(significand as i64),
                        step::Argument::I32(exponent),
                        step::Argument::I32(i32::from(integer)),
                        step::Argument::I32(i32::from(invalid)),
                    ],
                    ..step::Input::new(&[])
                };
                let (significand, exponent, coefficient) = if invalid {
                    (0xc000_0000_0000_0000, 0xffff, 0x3ff8_0000_0000_0000)
                } else if integer {
                    (significand, exponent, coefficient)
                } else {
                    (0xc000_0000_0000_0000, 0x4000, 0x3ff8_0000_0000_0000)
                };
                assert_eq!(
                    module.observe(&input, 1).events,
                    vec![step::Event::Return {
                        outcome: step::Outcome::Returned(vec![
                            step::Argument::I64(significand as i64),
                            step::Argument::I32(exponent),
                            step::Argument::I64(coefficient as i64)
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
