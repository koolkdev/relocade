//! A sign change keeps established representations and classification evidence.

use super::*;

#[test]
fn pc53_sign_changes_retain_native_and_encoded_significands() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::F64, Type::I64, Type::I16, Type::I1],
                results: vec![Type::I16],
            },
            |body| {
                let sign_exponent = body.parameter::<I16>(2)?;
                let native = ExtendedValue::from_precision53(Precision53::from_significand(
                    body.parameter::<F64>(0)?,
                    sign_exponent.clone(),
                ));
                let integer = ExtendedValue::from_bits(ExtendedBits {
                    significand: body.parameter::<I64>(1)?,
                    sign_exponent,
                })
                .assume_precision53();
                for source in [native, integer] {
                    let source = source
                        .assume_class(Classification::normal())
                        .or_indefinite(&body.parameter::<I1>(3)?);
                    for operation in [SignOperation::Negate, SignOperation::Absolute] {
                        let result = source.change_sign(operation);
                        assert!(result
                            .bits()
                            .significand
                            .same_expression(&source.bits().significand));
                        assert!(result
                            .precision53_significand()
                            .unwrap()
                            .same_expression(&source.precision53_significand().unwrap()));
                        assert!(result.tag().same_expression(&source.tag()));
                        assert!(result.normal().same_expression(&source.normal()));
                        assert!(result.nan().same_expression(&source.nan()));
                    }
                }
                body.return_(0_u32)
            },
        )
        .unwrap();
    program.export("preserved", function).unwrap();
    program.compile().unwrap();
}

#[test]
fn binary_sign_changes_retain_exact_source_format() {
    for (format, input, output) in [
        (BinaryFormat::Binary32, 0xbf80_0001_u64, 0x3f80_0001_u64),
        (BinaryFormat::Binary32, 0x8000_0000, 0),
        (BinaryFormat::Binary32, 0xffc0_0042, 0x7fc0_0042),
        (
            BinaryFormat::Binary64,
            0xbff0_0000_0000_0001,
            0x3ff0_0000_0000_0001,
        ),
        (BinaryFormat::Binary64, 0x8000_0000_0000_0000, 0),
        (
            BinaryFormat::Binary64,
            0xfff8_0000_0000_0042,
            0x7ff8_0000_0000_0042,
        ),
    ] {
        let source = ExtendedValue::from_binary(format, input.into());
        for operation in [SignOperation::Negate, SignOperation::Absolute] {
            let result = source.change_sign(operation);
            assert!(result
                .exact_bits(format)
                .unwrap()
                .same_expression(&output.into()));
            assert!(result.tag().same_expression(&source.tag()));
            assert!(result
                .bits()
                .significand
                .same_expression(&source.bits().significand));
        }
    }
}
