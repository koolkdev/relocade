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
fn known_normal_values_reuse_classification_after_selection() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I16, Type::I1],
                results: vec![Type::I16, Type::I1, Type::I32],
            },
            |body| {
                let normal = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(0)?,
                    sign_exponent: body.parameter::<I16>(1)?,
                })
                .assume_normal();
                let negative = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(0)?,
                    sign_exponent: body.parameter::<I16>(1)?.xor(0x8000),
                })
                .assume_normal();
                let value = normal.select(&body.parameter::<I1>(2)?, &negative);
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
                    "classification must not read the known-normal value's bits"
                );
            }
        }
    }
}

#[test]
fn indefinite_and_unknown_values_do_not_inherit_normal_classification() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I16, Type::I1, Type::I1],
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
                .assume_normal();
                let candidate = normal.or_indefinite(&body.parameter::<I1>(2)?);
                let unknown = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(0)?,
                    sign_exponent: body.parameter::<I16>(1)?,
                });
                let value = candidate.select(&body.parameter::<I1>(3)?, &unknown);
                let known = candidate.select(&body.parameter::<I1>(3)?, &normal);
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
        segment_profile: None,
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
        for invalid in [false, true] {
            for use_candidate in [false, true] {
                let (tag, classes) = match (use_candidate, invalid) {
                    (true, false) => (0, 0),
                    (true, true) => (2, 0b0010100),
                    (false, _) => (tag, classes),
                };
                let input = step::Input {
                    arguments: vec![
                        step::Argument::I64(significand as i64),
                        step::Argument::I32(exponent),
                        step::Argument::I32(i32::from(invalid)),
                        step::Argument::I32(i32::from(use_candidate)),
                    ],
                    ..step::Input::new(&[])
                };
                let known_indefinite = invalid && use_candidate;
                let expected = vec![step::Event::Return {
                    outcome: step::Outcome::Returned(vec![
                        step::Argument::I32(tag),
                        step::Argument::I32(i32::from(tag == 0)),
                        step::Argument::I32(classes),
                        step::Argument::I32(if known_indefinite { 2 } else { 0 }),
                        step::Argument::I32(i32::from(!known_indefinite)),
                        step::Argument::I32(if known_indefinite { 0b0010100 } else { 0 }),
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
