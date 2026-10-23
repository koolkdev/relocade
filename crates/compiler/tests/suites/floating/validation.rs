//! Type validation and emitted operations protect the floating-point boundary.

use super::*;
use relocade_compiler::{Argument, BuildError, Program};
use wasmparser::{Operator, Parser, Payload};

fn operators(module: &TestModule) -> Vec<Operator<'_>> {
    Parser::new(0)
        .parse_all(module.bytes())
        .flat_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => body
                .get_operators_reader()
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect(),
            _ => vec![],
        })
        .collect()
}

#[test]
fn signatures_distinguish_float_literals_from_integer_encodings() {
    for (argument, expected, actual) in [
        (Argument::from(1), Type::F64, Type::I32),
        (Argument::from(1_u32), Type::F64, Type::I32),
        (Argument::from(1_u64), Type::F64, Type::I64),
        (Argument::from(1.0), Type::I64, Type::F64),
        (Argument::from(1.0), Type::I1, Type::F64),
    ] {
        let result =
            Program::new().raw_function(signature(&[], &[expected]), |body| body.return_(argument));
        assert_eq!(
            result.err(),
            Some(BuildError::TypeMismatch { expected, actual })
        );
    }
    let mismatch = Program::new().function::<fn(f64) -> u64>(|body, _| {
        let value = body.parameter::<I64>(0)?;
        body.return_(value)
    });
    assert_eq!(
        mismatch.err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I64,
            actual: Type::F64
        })
    );
}

#[test]
fn scalar_float_operations_lower_directly_and_inverse_bitcasts_disappear() {
    let module = Fixture::new().expression(&[Type::F64; 2], |body| {
        let a = body.parameter::<F64>(0).unwrap();
        let b = body.parameter::<F64>(1).unwrap();
        a.add(&b).sub(&b).mul(&b).div(&b).abs().neg().to_bits()
    });
    let ops = operators(&module);
    for predicate in [
        (|op: &Operator<'_>| matches!(op, Operator::F64Add)) as fn(&Operator<'_>) -> bool,
        |op| matches!(op, Operator::F64Sub),
        |op| matches!(op, Operator::F64Mul),
        |op| matches!(op, Operator::F64Div),
        |op| matches!(op, Operator::F64Abs),
        |op| matches!(op, Operator::F64Neg),
    ] {
        assert_eq!(ops.iter().filter(|op| predicate(op)).count(), 1);
    }

    let inverse = Fixture::new().expression(&[Type::I64], |body| {
        Val::<F64>::from_bits(body.parameter::<I64>(0).unwrap()).to_bits()
    });
    assert!(matches!(
        operators(&inverse).as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::Return,
            Operator::End
        ]
    ));
    let inverse = Fixture::new().expression(&[Type::F64], |body| {
        Val::<F64>::from_bits(body.parameter::<F64>(0).unwrap().to_bits())
    });
    assert!(matches!(
        operators(&inverse).as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::Return,
            Operator::End
        ]
    ));
}

#[test]
fn literal_identity_keeps_encodings_and_arithmetic_nan_choice_stays_in_wasm() {
    assert!(!Val::<F64>::from(0.0).same_expression(&Val::from(-0.0)));
    assert!(!Val::<F64>::from_bits(NAN).same_expression(&Val::<F64>::from_bits(NAN + 1)));
    let literal = Fixture::new().expression(&[], |_| Val::<F64>::from(1.5).mul(2.0));
    assert!(
        matches!(operators(&literal).as_slice(), [Operator::F64Const { value }, Operator::Return, Operator::End] if value.bits() == 3.0_f64.to_bits())
    );
    let invalid = Fixture::new().expression(&[], |_| Val::<F64>::from(0.0).div(0.0));
    assert!(operators(&invalid)
        .iter()
        .any(|op| matches!(op, Operator::F64Div)));
}

#[test]
fn rounding_and_square_root_lower_directly_and_defer_arithmetic_nans() {
    let module = Fixture::new().function(&[Type::F64], &[Type::F64; 5], |body| {
        let value = body.parameter::<F64>(0)?;
        body.return_([
            value.ceil(),
            value.floor(),
            value.trunc(),
            value.nearest(),
            value.sqrt(),
        ])
    });
    let ops = operators(&module);
    for predicate in [
        (|op: &Operator<'_>| matches!(op, Operator::F64Ceil)) as fn(&Operator<'_>) -> bool,
        |op| matches!(op, Operator::F64Floor),
        |op| matches!(op, Operator::F64Trunc),
        |op| matches!(op, Operator::F64Nearest),
        |op| matches!(op, Operator::F64Sqrt),
    ] {
        assert_eq!(ops.iter().filter(|op| predicate(op)).count(), 1);
    }
    for value in [
        Val::<F64>::from(2.5).nearest(),
        Val::<F64>::from(4.0).sqrt(),
    ] {
        let constant = Fixture::new().expression(&[], |_| value);
        assert!(
            matches!(operators(&constant).as_slice(), [Operator::F64Const { value }, Operator::Return, Operator::End] if value.bits() == 2.0_f64.to_bits())
        );
    }
    let nan = Val::<F64>::from_bits(INFINITY | 1);
    let special = Fixture::new().function(&[], &[Type::F64; 6], |body| {
        body.return_([
            nan.ceil(),
            nan.floor(),
            nan.trunc(),
            nan.nearest(),
            nan.sqrt(),
            Val::<F64>::from(-1.0).sqrt(),
        ])
    });
    assert_eq!(
        operators(&special)
            .iter()
            .filter(|op| matches!(op, Operator::F64Sqrt))
            .count(),
        2
    );
    assert!(operators(&special)
        .iter()
        .any(|op| matches!(op, Operator::F64Nearest)));
}

#[test]
fn numeric_conversions_emit_native_operations_and_fold_exact_constants() {
    let demotion = Fixture::new().expression(&[Type::F64], |body| {
        body.parameter::<F64>(0).unwrap().to_binary32_bits()
    });
    assert!(matches!(
        operators(&demotion).as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::F32DemoteF64,
            Operator::I32ReinterpretF32,
            Operator::Return,
            Operator::End
        ]
    ));
    let constant = Fixture::new().expression(&[], |_| Val::<F64>::from(1.0).to_binary32_bits());
    assert!(matches!(
        operators(&constant).as_slice(),
        [
            Operator::I32Const { value: 0x3f80_0000 },
            Operator::Return,
            Operator::End
        ]
    ));
    let nan = Fixture::new().expression(&[], |_| {
        Val::<F64>::from_bits(0x7ff0_0000_0000_0001_u64).to_binary32_bits()
    });
    assert!(operators(&nan)
        .iter()
        .any(|op| matches!(op, Operator::F32DemoteF64)));
    let promotion = Fixture::new().expression(&[Type::I32], |body| {
        Val::<F64>::from_binary32_bits(body.parameter::<I32>(0).unwrap())
    });
    assert!(matches!(
        operators(&promotion).as_slice(),
        [
            Operator::LocalGet { local_index: 0 },
            Operator::F32ReinterpretI32,
            Operator::F64PromoteF32,
            Operator::Return,
            Operator::End
        ]
    ));
    let integers = Fixture::new().function(&[Type::I32, Type::I64], &[Type::F64; 4], |body| {
        let word = body.parameter::<I32>(0)?;
        let wide = body.parameter::<I64>(1)?;
        body.return_([
            word.signed().to_f64(),
            word.unsigned().to_f64(),
            wide.signed().to_f64(),
            wide.unsigned().to_f64(),
        ])
    });
    let ops = operators(&integers);
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::F64ConvertI32S))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::F64ConvertI32U))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::F64ConvertI64S))
            .count(),
        1
    );
    assert_eq!(
        ops.iter()
            .filter(|op| matches!(op, Operator::F64ConvertI64U))
            .count(),
        1
    );
    for value in [
        Val::<F64>::from_binary32_bits(0x3f80_0000_u32),
        Val::<I32>::from(1).signed().to_f64(),
    ] {
        let module = Fixture::new().expression(&[], |_| value);
        assert!(
            matches!(operators(&module).as_slice(), [Operator::F64Const { value }, Operator::Return, Operator::End] if value.bits() == 1.0_f64.to_bits())
        );
    }
    let nan = Fixture::new().expression(&[], |_| Val::<F64>::from_binary32_bits(0x7f80_0001_u32));
    assert!(operators(&nan)
        .iter()
        .any(|op| matches!(op, Operator::F64PromoteF32)));
}
