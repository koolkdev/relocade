//! Floating operations retain their semantics through folding and Wasm lowering.

use super::*;
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
fn floating_views_lower_to_native_simd() {
    let module = Fixture::new().function(&[Type::V128; 2], &[Type::V128; 12], move |body| {
        let a = body.parameter::<V128>(0)?;
        let b = body.parameter::<V128>(1)?;
        body.return_([
            a.f32x4()
                .add(&b)
                .f32x4()
                .sub(&b)
                .f32x4()
                .mul(&b)
                .f32x4()
                .div(&b)
                .f32x4()
                .abs()
                .f32x4()
                .neg()
                .f32x4()
                .ceil()
                .f32x4()
                .floor()
                .f32x4()
                .trunc()
                .f32x4()
                .nearest()
                .f32x4()
                .sqrt(),
            a.f64x2()
                .add(&b)
                .f64x2()
                .sub(&b)
                .f64x2()
                .mul(&b)
                .f64x2()
                .div(&b)
                .f64x2()
                .abs()
                .f64x2()
                .neg()
                .f64x2()
                .ceil()
                .f64x2()
                .floor()
                .f64x2()
                .trunc()
                .f64x2()
                .nearest()
                .f64x2()
                .sqrt(),
            a.f32x4().eq(&b).f32x4().ne(&b).f32x4().lt(&b),
            a.f32x4().le(&b).f32x4().gt(&b).f32x4().ge(&b),
            a.f64x2().eq(&b).f64x2().ne(&b).f64x2().lt(&b),
            a.f64x2().le(&b).f64x2().gt(&b).f64x2().ge(&b),
            a.f32x4().promote_low(),
            a.f64x2().demote_zero(),
            a.signed_i32_lanes_to_f32(),
            a.low_signed_i32_lanes_to_f64(),
            a.f32x4().trunc_saturating_i32(),
            a.f64x2().trunc_saturating_i32_zero(),
        ])
    });
    let ops = operators(&module);
    macro_rules! once {
        ($($op:ident),+ $(,)?) => { $(
            assert_eq!(ops.iter().filter(|op| matches!(op, Operator::$op)).count(), 1, stringify!($op));
        )+ };
    }
    once!(
        F32x4Add,
        F32x4Sub,
        F32x4Mul,
        F32x4Div,
        F32x4Abs,
        F32x4Neg,
        F32x4Ceil,
        F32x4Floor,
        F32x4Trunc,
        F32x4Nearest,
        F32x4Sqrt,
        F64x2Add,
        F64x2Sub,
        F64x2Mul,
        F64x2Div,
        F64x2Abs,
        F64x2Neg,
        F64x2Ceil,
        F64x2Floor,
        F64x2Trunc,
        F64x2Nearest,
        F64x2Sqrt,
        F32x4Eq,
        F32x4Ne,
        F32x4Lt,
        F32x4Le,
        F32x4Gt,
        F32x4Ge,
        F64x2Eq,
        F64x2Ne,
        F64x2Lt,
        F64x2Le,
        F64x2Gt,
        F64x2Ge,
        F64x2PromoteLowF32x4,
        F32x4DemoteF64x2Zero,
        F32x4ConvertI32x4S,
        F64x2ConvertLowI32x4S,
        I32x4TruncSatF32x4S,
        I32x4TruncSatF64x2SZero,
    );
}

#[test]
fn finite_literals_fold_but_nan_results_stay_in_wasm() {
    let folded = Fixture::new().expression(&[], |_| {
        Val::<V128>::from(words([0x3fc0_0000; 4]))
            .f32x4()
            .mul(words([0x4000_0000; 4]))
    });
    assert!(matches!(operators(&folded).as_slice(),
        [Operator::V128Const { value }, Operator::Return, Operator::End]
        if value.bytes() == &words([0x4040_0000; 4]).to_le_bytes()));
    let invalid = Fixture::new().expression(&[], |_| Val::<V128>::from(0_u128).f32x4().div(0_u128));
    assert!(operators(&invalid)
        .iter()
        .any(|op| matches!(op, Operator::F32x4Div)));

    let identities = Fixture::new().function(&[Type::V128], &[Type::V128; 3], move |body| {
        let a = body.parameter::<V128>(0)?;
        body.return_([a.f32x4().sub(&a), a.f64x2().eq(&a), a.f32x4().add(0_u128)])
    });
    let ops = operators(&identities);
    assert!(ops.iter().any(|op| matches!(op, Operator::F32x4Sub)));
    assert!(ops.iter().any(|op| matches!(op, Operator::F64x2Eq)));
    assert!(ops.iter().any(|op| matches!(op, Operator::F32x4Add)));
}
