use super::*;
use crate::test_step as step;
use crate::x87::ExtendedValue;
use relocade_compiler::{Program, Signature, Type, I32};
use wasmparser::{Operator, Parser, Payload};

fn non_normal_classes(value: &ExtendedValue) -> [Val<I1>; 7] {
    let class = value.classification();
    [
        class.zero(),
        class.denormal(),
        class.special_exponent(),
        class.infinity(),
        class.nan(),
        class.signaling_nan(),
        class.unsupported(),
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
                results: vec![Type::I16, Type::I1, Type::I32, Type::I32],
            },
            |body| {
                let normal = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(0)?,
                    sign_exponent: body.parameter::<I16>(1)?,
                })
                .assume_class(KnownClass::normal());
                let loaded = BinaryFormat::Binary64
                    .decode(&body.parameter::<I64>(0)?)
                    .loaded_value()
                    .assume_class(KnownClass::normal());
                let zero = ExtendedValue::from_bits(ExtendedBits {
                    significand: 0_u64.into(),
                    sign_exponent: 0x8000.into(),
                })
                .assume_class(KnownClass::zero());
                let value = normal
                    .select(&body.parameter::<I1>(2)?, &loaded)
                    .select(&body.parameter::<I1>(3)?, &zero);
                body.return_((
                    value.classification().tag(),
                    value.classification().normal(),
                    class_flags(&value),
                    class_flags(&loaded),
                ))
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
                .assume_class(KnownClass::normal());
                let zero = ExtendedValue::from_bits(ExtendedBits {
                    significand: 0_u64.into(),
                    sign_exponent: 0x8000.into(),
                })
                .assume_class(KnownClass::zero());
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
                    value.classification().tag(),
                    value.classification().normal(),
                    class_flags(&value),
                    known.classification().tag(),
                    known.classification().normal(),
                    class_flags(&known),
                ))
            },
        )
        .unwrap();
    program.export("classify", function).unwrap();
    let module = step::TestModule::from_bytes(&program.compile().unwrap(), "classify");
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
        (0, 0x7fff, 2, 0b1001100),
        (1, 0x7fff, 2, 0b1110100),
        (1_u64 << 62, 0x7fff, 2, 0b1010100),
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
fn loaded_binary_values_share_classification_across_exact_representations() {
    struct Case {
        source_bits: u64,
        loaded_tag: i32,
        loaded_classes: i32,
        source_exceptions: i32,
        source_classes: i32,
        loaded_bits: u64,
    }

    for (format, cases) in [
        (
            BinaryFormat::Binary32,
            vec![
                Case {
                    source_bits: 0_u64,
                    loaded_tag: 1,
                    loaded_classes: 0x101,
                    source_exceptions: 0,
                    source_classes: 0x3,
                    loaded_bits: 0_u64,
                },
                Case {
                    source_bits: 0x8000_0000,
                    loaded_tag: 1,
                    loaded_classes: 0x101,
                    source_exceptions: 0,
                    source_classes: 0x3,
                    loaded_bits: 0x8000_0000,
                },
                Case {
                    source_bits: 1,
                    loaded_tag: 0,
                    loaded_classes: 0x180,
                    source_exceptions: 1,
                    source_classes: 0x2,
                    loaded_bits: 1,
                },
                Case {
                    source_bits: 0x007f_ffff,
                    loaded_tag: 0,
                    loaded_classes: 0x180,
                    source_exceptions: 1,
                    source_classes: 0x2,
                    loaded_bits: 0x007f_ffff,
                },
                Case {
                    source_bits: 0x0080_0000,
                    loaded_tag: 0,
                    loaded_classes: 0x180,
                    source_exceptions: 0,
                    source_classes: 0x2,
                    loaded_bits: 0x0080_0000,
                },
                Case {
                    source_bits: 0xff7f_ffff,
                    loaded_tag: 0,
                    loaded_classes: 0x180,
                    source_exceptions: 0,
                    source_classes: 0x2,
                    loaded_bits: 0xff7f_ffff,
                },
                Case {
                    source_bits: 0x7f80_0000,
                    loaded_tag: 2,
                    loaded_classes: 0xc,
                    source_exceptions: 0,
                    source_classes: 0xc,
                    loaded_bits: 0x7f80_0000,
                },
                Case {
                    source_bits: 0xff80_0000,
                    loaded_tag: 2,
                    loaded_classes: 0xc,
                    source_exceptions: 0,
                    source_classes: 0xc,
                    loaded_bits: 0xff80_0000,
                },
                Case {
                    source_bits: 0x7fc0_0001,
                    loaded_tag: 2,
                    loaded_classes: 0x14,
                    source_exceptions: 0,
                    source_classes: 0x14,
                    loaded_bits: 0x7fc0_0001,
                },
                Case {
                    source_bits: 0x7f80_0001,
                    loaded_tag: 2,
                    loaded_classes: 0x14,
                    source_exceptions: 2,
                    source_classes: 0x14,
                    loaded_bits: 0x7fc0_0001,
                },
            ],
        ),
        (
            BinaryFormat::Binary64,
            vec![
                Case {
                    source_bits: 0,
                    loaded_tag: 1,
                    loaded_classes: 0x101,
                    source_exceptions: 0,
                    source_classes: 0x3,
                    loaded_bits: 0,
                },
                Case {
                    source_bits: 0x8000_0000_0000_0000,
                    loaded_tag: 1,
                    loaded_classes: 0x101,
                    source_exceptions: 0,
                    source_classes: 0x3,
                    loaded_bits: 0x8000_0000_0000_0000,
                },
                Case {
                    source_bits: 1,
                    loaded_tag: 0,
                    loaded_classes: 0x180,
                    source_exceptions: 1,
                    source_classes: 0x2,
                    loaded_bits: 1,
                },
                Case {
                    source_bits: 0x000f_ffff_ffff_ffff,
                    loaded_tag: 0,
                    loaded_classes: 0x180,
                    source_exceptions: 1,
                    source_classes: 0x2,
                    loaded_bits: 0x000f_ffff_ffff_ffff,
                },
                Case {
                    source_bits: 0x0010_0000_0000_0000,
                    loaded_tag: 0,
                    loaded_classes: 0x180,
                    source_exceptions: 0,
                    source_classes: 0x2,
                    loaded_bits: 0x0010_0000_0000_0000,
                },
                Case {
                    source_bits: 0xffef_ffff_ffff_ffff,
                    loaded_tag: 0,
                    loaded_classes: 0x180,
                    source_exceptions: 0,
                    source_classes: 0x2,
                    loaded_bits: 0xffef_ffff_ffff_ffff,
                },
                Case {
                    source_bits: 0x7ff0_0000_0000_0000,
                    loaded_tag: 2,
                    loaded_classes: 0xc,
                    source_exceptions: 0,
                    source_classes: 0xc,
                    loaded_bits: 0x7ff0_0000_0000_0000,
                },
                Case {
                    source_bits: 0xfff0_0000_0000_0000,
                    loaded_tag: 2,
                    loaded_classes: 0xc,
                    source_exceptions: 0,
                    source_classes: 0xc,
                    loaded_bits: 0xfff0_0000_0000_0000,
                },
                Case {
                    source_bits: 0x7ff8_0000_0000_0001,
                    loaded_tag: 2,
                    loaded_classes: 0x14,
                    source_exceptions: 0,
                    source_classes: 0x14,
                    loaded_bits: 0x7ff8_0000_0000_0001,
                },
                Case {
                    source_bits: 0x7ff0_0000_0000_0001,
                    loaded_tag: 2,
                    loaded_classes: 0x14,
                    source_exceptions: 2,
                    source_classes: 0x14,
                    loaded_bits: 0x7ff8_0000_0000_0001,
                },
            ],
        ),
    ] {
        let mut program = Program::new();
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I64],
                    results: vec![
                        Type::I32,
                        Type::I32,
                        Type::I32,
                        Type::I16,
                        Type::I32,
                        Type::I32,
                        Type::I64,
                    ],
                },
                |body| {
                    let source = format.decode(&body.parameter::<I64>(0)?);
                    let raw = source.classification();
                    let source_classes = raw
                        .zero()
                        .unsigned()
                        .extend::<I32>()
                        .or(raw.finite().unsigned().extend::<I32>().shl(1))
                        .or(raw.special_exponent().unsigned().extend::<I32>().shl(2))
                        .or(raw.infinity().unsigned().extend::<I32>().shl(3))
                        .or(raw.nan().unsigned().extend::<I32>().shl(4));
                    let value = source.loaded_value();
                    let expanded = ExtendedValue::from_bits(value.bits());
                    let precision53 = value.clone().assume_precision53();
                    let flags = |value: &ExtendedValue| {
                        let class = value.classification();
                        class_flags(value)
                            .or(class.normal().unsigned().extend::<I32>().shl(7))
                            .or(class.normal_or_zero().unsigned().extend::<I32>().shl(8))
                    };
                    let exceptions = source.denormal.unsigned().extend::<I32>().or(source
                        .signaling_nan
                        .unsigned()
                        .extend::<I32>()
                        .shl(1));
                    body.return_((
                        flags(&value),
                        flags(&expanded),
                        flags(&precision53),
                        value.classification().tag(),
                        exceptions,
                        source_classes,
                        value.exact_conversion(format).unwrap().bits,
                    ))
                },
            )
            .unwrap();
        program.export("classify", function).unwrap();
        let module = step::TestModule::from_bytes(&program.compile().unwrap(), "classify");
        // Predicate bits match class_flags, followed by normal and normal-or-zero.
        // Source classification bits: zero, finite, special exponent, infinity, NaN.
        // Exception bits separately record #D and SNaN evidence before loading.
        for case in cases {
            let input = step::Input {
                arguments: vec![step::Argument::I64(case.source_bits as i64)],
                ..step::Input::new(&[])
            };
            assert_eq!(
                module.observe(&input, 1).events,
                vec![step::Event::Return {
                    outcome: step::Outcome::Returned(vec![
                        step::Argument::I32(case.loaded_classes),
                        step::Argument::I32(case.loaded_classes),
                        step::Argument::I32(case.loaded_classes),
                        step::Argument::I32(case.loaded_tag),
                        step::Argument::I32(case.source_exceptions),
                        step::Argument::I32(case.source_classes),
                        step::Argument::I64(case.loaded_bits as i64),
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
