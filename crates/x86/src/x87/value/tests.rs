use super::*;
use crate::{test_step as step, CompiledModule};
use relocade_compiler::{Program, Signature, Type};

mod binary_class;
mod sign;

#[test]
fn precision53_views_agree_after_selection_and_indefinite_replacement() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I16, Type::I1, Type::I1],
                results: vec![
                    Type::I64,
                    Type::I16,
                    Type::I64,
                    Type::I16,
                    Type::I1,
                    Type::I1,
                ],
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
                    value.classification().tag(),
                    value.classification().infinity(),
                    value.classification().nan(),
                ))
            },
        )
        .unwrap();
    program.export("views", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "views".into(),
        execution_target: None,
    });
    for (significand, exponent, coefficient, tag, infinity) in [
        (0_u64, 0x8000, 0_u64, 1, 0),
        (0xa000_0000_0000_0000, 1, 0x3ff4_0000_0000_0000, 0, 0),
        (0xffff_ffff_ffff_f800, 0xfffe, 0x3fff_ffff_ffff_ffff, 0, 0),
        (0x8000_0000_0000_0000, 0x7fff, 0x3ff0_0000_0000_0000, 2, 1),
        (0x8000_0000_0000_0000, 0, 0x3ff0_0000_0000_0000, 2, 0),
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
                let (significand, exponent, coefficient, tag, infinity) = if invalid {
                    (0xc000_0000_0000_0000, 0xffff, 0x3ff8_0000_0000_0000, 2, 0)
                } else if integer {
                    (significand, exponent, coefficient, tag, infinity)
                } else {
                    (0xc000_0000_0000_0000, 0x4000, 0x3ff8_0000_0000_0000, 0, 0)
                };
                assert_eq!(
                    module.observe(&input, 1).events,
                    vec![step::Event::Return {
                        outcome: step::Outcome::Returned(vec![
                            step::Argument::I64(significand as i64),
                            step::Argument::I32(exponent),
                            step::Argument::I64(coefficient as i64),
                            step::Argument::I32(tag),
                            step::Argument::I32(infinity),
                            step::Argument::I32(i32::from(invalid))
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

#[test]
fn exact_binary64_views_follow_selection_and_indefinite_replacement() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I1, Type::I1],
                results: vec![Type::I64, Type::I16, Type::I64, Type::I64, Type::I64],
            },
            |body| {
                let integer = ExtendedValue::from_signed_integer(&body.parameter::<I32>(0)?);
                let other = ExtendedValue::from_signed_integer(&Val::<I32>::from(-3));
                let value = integer
                    .select(&body.parameter::<I1>(1)?, &other)
                    .or_indefinite(&body.parameter::<I1>(2)?);
                let bits = value.bits();
                // A matching significand precision does not establish the
                // whole value's range. Selecting such a value drops the view.
                let wide = ExtendedValue::from_precision53(Precision53::from_significand(
                    1.0.into(),
                    0x7ffe.into(),
                ));
                assert!(wide.binary64_value().is_none());
                assert!(value
                    .select(&body.parameter::<I1>(1)?, &wide)
                    .binary64_value()
                    .is_none());
                body.return_((
                    bits.significand,
                    bits.sign_exponent,
                    value.binary64_value().unwrap().to_bits(),
                    value
                        .change_sign(SignOperation::Negate)
                        .binary64_value()
                        .unwrap()
                        .to_bits(),
                    value
                        .change_sign(SignOperation::Absolute)
                        .binary64_value()
                        .unwrap()
                        .to_bits(),
                ))
            },
        )
        .unwrap();
    program.export("views", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "views".into(),
        execution_target: None,
    });
    for (integer, significand, exponent, native) in [
        (0, 0_u64, 0, 0_u64),
        (1, 0x8000_0000_0000_0000, 0x3fff, 0x3ff0_0000_0000_0000),
        (
            i32::MIN,
            0x8000_0000_0000_0000,
            0xc01e,
            0xc1e0_0000_0000_0000,
        ),
        (
            i32::MAX,
            0xffff_fffe_0000_0000,
            0x401d,
            0x41df_ffff_ffc0_0000,
        ),
    ] {
        for take_integer in [false, true] {
            for invalid in [false, true] {
                let input = step::Input {
                    arguments: vec![
                        step::Argument::I32(integer),
                        step::Argument::I32(i32::from(take_integer)),
                        step::Argument::I32(i32::from(invalid)),
                    ],
                    ..step::Input::new(&[])
                };
                let (significand, exponent, native) = if invalid {
                    (0xc000_0000_0000_0000, 0xffff, 0xfff8_0000_0000_0000)
                } else if take_integer {
                    (significand, exponent, native)
                } else {
                    (0xc000_0000_0000_0000, 0xc000, 0xc008_0000_0000_0000)
                };
                assert_eq!(
                    module.observe(&input, 1).events,
                    vec![step::Event::Return {
                        outcome: step::Outcome::Returned(vec![
                            step::Argument::I64(significand as i64),
                            step::Argument::I32(exponent),
                            step::Argument::I64(native as i64),
                            step::Argument::I64((native ^ (1 << 63)) as i64),
                            step::Argument::I64((native & !(1 << 63)) as i64)
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
