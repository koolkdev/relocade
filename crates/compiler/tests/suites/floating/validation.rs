//! Type validation and emitted operations protect the floating-point boundary.

use super::*;
use wasm86_compiler::{Argument, BuildError, Program};
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
            Program::new().function(signature(&[], &[expected]), |body| body.return_(argument));
        assert_eq!(
            result.err(),
            Some(BuildError::TypeMismatch { expected, actual })
        );
    }
    let mismatch = Program::new().function(signature(&[Type::F64], &[Type::I64]), |body| {
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
    assert!(!Val::<F64>::from_bits(NAN).same_expression(&Val::from_bits(NAN + 1)));
    let literal = Fixture::new().expression(&[], |_| Val::<F64>::from(1.5).mul(2.0));
    assert!(
        matches!(operators(&literal).as_slice(), [Operator::F64Const { value }, Operator::Return, Operator::End] if value.bits() == 3.0_f64.to_bits())
    );
    let invalid = Fixture::new().expression(&[], |_| Val::<F64>::from(0.0).div(0.0));
    assert!(operators(&invalid)
        .iter()
        .any(|op| matches!(op, Operator::F64Div)));
}
