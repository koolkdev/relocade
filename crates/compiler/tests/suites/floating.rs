//! Strict binary64 semantics and exact encoding transport use the ordinary compiler API.

#[path = "floating/flow.rs"]
mod flow;
#[path = "floating/validation.rs"]
mod validation;

use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, Observation, TestModule, Value};
use wasm86_compiler::{Type, Val, F64, I1, I32, I64};

const SIGN: u64 = 1 << 63;
const INFINITY: u64 = 0x7ff0_0000_0000_0000;
const NAN: u64 = 0x7ff8_0000_0000_0000;

fn check(module: &TestModule, arguments: &[Value], expected: &[Value], v8: bool) {
    if v8 {
        assert_eq!(
            module.run_v8(&Input::call("run", arguments)),
            Observation::returned(expected)
        );
    } else {
        assert_eq!(
            module.instantiate().call_values("run", arguments).unwrap(),
            expected
        );
    }
}

// Arithmetic NaN payloads are engine choices; transport tests compare exact bits.
fn observed_bits(value: Val<F64>) -> Val<I64> {
    let bits = value.to_bits();
    bits.and(!SIGN)
        .unsigned()
        .ge(INFINITY + 1)
        .select(NAN, bits)
}

fn arithmetic(v8: bool) {
    let runtime = Fixture::new().function(&[Type::I64; 2], &[Type::I64; 4], |body| {
        let a = Val::<F64>::from_bits(body.parameter::<I64>(0)?);
        let b = Val::<F64>::from_bits(body.parameter::<I64>(1)?);
        body.return_([a.add(&b), a.sub(&b), a.mul(&b), a.div(&b)].map(observed_bits))
    });
    // Expected encodings are literal IEEE results, including tie and range boundaries.
    for (a, b, expected) in [
        (
            1.5_f64.to_bits(),
            2.0_f64.to_bits(),
            [
                3.5_f64.to_bits(),
                (-0.5_f64).to_bits(),
                3.0_f64.to_bits(),
                0.75_f64.to_bits(),
            ],
        ),
        (0, SIGN, [0, 0, SIGN, NAN]),
        (SIGN, SIGN, [SIGN, 0, 0, NAN]),
        (INFINITY, INFINITY, [INFINITY, NAN, INFINITY, NAN]),
        (0x7ff0_0000_0000_0042, 1.0_f64.to_bits(), [NAN; 4]),
        (1, 0, [1, 1, 0, INFINITY]),
        (
            0x0010_0000_0000_0000,
            0.5_f64.to_bits(),
            [
                0.5_f64.to_bits(),
                (-0.5_f64).to_bits(),
                0x0008_0000_0000_0000,
                0x0020_0000_0000_0000,
            ],
        ),
        (
            1.0_f64.to_bits(),
            0x3ca0_0000_0000_0000,
            [
                1.0_f64.to_bits(),
                0x3fef_ffff_ffff_ffff,
                0x3ca0_0000_0000_0000,
                0x4340_0000_0000_0000,
            ],
        ),
    ] {
        let expected = expected.map(|bits| Value::I64(bits as i64));
        check(
            &runtime,
            &[Value::I64(a as i64), Value::I64(b as i64)],
            &expected,
            v8,
        );
        for admitted in [false, true] {
            let constants = Fixture::new().function(&[], &[Type::I64; 4], |body| {
                let a = Val::<F64>::from_bits(a);
                let b = Val::<F64>::from_bits(b);
                let (a, b) = if admitted {
                    (body.value(a)?, body.value(b)?)
                } else {
                    (a, b)
                };
                body.return_([a.add(&b), a.sub(&b), a.mul(&b), a.div(&b)].map(observed_bits))
            });
            check(&constants, &[], &expected, v8);
        }
    }
}

fn rounding_and_order(v8: bool) {
    let module = Fixture::new().function(&[Type::I64; 2], &[Type::I64; 4], |body| {
        let a = Val::<F64>::from_bits(body.parameter::<I64>(0)?);
        let b = Val::<F64>::from_bits(body.parameter::<I64>(1)?);
        body.return_([
            observed_bits(a.mul(&b)),
            observed_bits(a.add(1.0).sub(&a)),
            observed_bits(a.mul(&b).sub(1.0)),
            observed_bits(a.sub(&a)),
        ])
    });
    for (a, b, expected) in [
        (
            0x0010_0000_0000_0000,
            0.5_f64.to_bits(),
            [
                0x0008_0000_0000_0000,
                1.0_f64.to_bits(),
                (-1.0_f64).to_bits(),
                0,
            ],
        ),
        (
            1,
            0.5_f64.to_bits(),
            [0, 1.0_f64.to_bits(), (-1.0_f64).to_bits(), 0],
        ),
        (
            SIGN | 1,
            0.5_f64.to_bits(),
            [SIGN, 1.0_f64.to_bits(), (-1.0_f64).to_bits(), 0],
        ),
        (
            0x4340_0000_0000_0000,
            1.0_f64.to_bits(),
            [0x4340_0000_0000_0000, 0, 0x433f_ffff_ffff_ffff, 0],
        ),
        (
            0x3ff0_0000_0200_0000,
            0x3fef_ffff_fc00_0000,
            [1.0_f64.to_bits(), 1.0_f64.to_bits(), 0, 0],
        ),
        (INFINITY, 0, [NAN; 4]),
    ] {
        check(
            &module,
            &[Value::I64(a as i64), Value::I64(b as i64)],
            &expected.map(|bits| Value::I64(bits as i64)),
            v8,
        );
    }
}

fn comparisons(v8: bool) {
    let module = Fixture::new().function(&[Type::I64; 2], &[Type::I1; 9], |body| {
        let a = Val::<F64>::from_bits(body.parameter::<I64>(0)?);
        let b = Val::<F64>::from_bits(body.parameter::<I64>(1)?);
        body.return_([
            a.eq(&b),
            a.ne(&b),
            a.lt(&b),
            a.le(&b),
            a.gt(&b),
            a.ge(&b),
            a.eq(&a),
            a.ne(&a),
            a.lt(&b).eq(false),
        ])
    });
    for (a, b, expected) in [
        (0, SIGN, [1, 0, 0, 1, 0, 1, 1, 0, 1]),
        (
            1.0_f64.to_bits(),
            2.0_f64.to_bits(),
            [0, 1, 1, 1, 0, 0, 1, 0, 0],
        ),
        (INFINITY, 2.0_f64.to_bits(), [0, 1, 0, 0, 1, 1, 1, 0, 1]),
        (NAN, 0, [0, 1, 0, 0, 0, 0, 0, 1, 1]),
        (0, NAN, [0, 1, 0, 0, 0, 0, 1, 0, 1]),
        (
            0x7ff0_0000_0000_0042,
            0x7ff0_0000_0000_0042,
            [0, 1, 0, 0, 0, 0, 0, 1, 1],
        ),
    ] {
        check(
            &module,
            &[Value::I64(a as i64), Value::I64(b as i64)],
            &expected.map(Value::I32),
            v8,
        );
        let constants = Fixture::new().function(&[], &[Type::I1; 9], |body| {
            let a = Val::<F64>::from_bits(a);
            let b = Val::<F64>::from_bits(b);
            body.return_([
                a.eq(&b),
                a.ne(&b),
                a.lt(&b),
                a.le(&b),
                a.gt(&b),
                a.ge(&b),
                a.eq(&a),
                a.ne(&a),
                a.lt(&b).eq(false),
            ])
        });
        check(&constants, &[], &expected.map(Value::I32), v8);
    }
    let guarded = Fixture::new().function(&[Type::I64; 2], &[Type::I64], |mut body| {
        let a = Val::<F64>::from_bits(body.parameter::<I64>(0)?);
        let b = Val::<F64>::from_bits(body.parameter::<I64>(1)?);
        body.if_(a.eq(&b), |arm| arm.return_(a.to_bits().xor(b.to_bits())))?;
        body.if_(a.lt(&b).eq(false), |arm| {
            arm.return_(a.ge(&b).unsigned().extend::<I64>())
        })?;
        body.return_(9_u64)
    });
    check(
        &guarded,
        &[Value::I64(0), Value::I64(SIGN as i64)],
        &[Value::I64(SIGN as i64)],
        v8,
    );
    check(
        &guarded,
        &[Value::I64(NAN as i64), Value::I64(0)],
        &[Value::I64(0)],
        v8,
    );
}

#[test]
fn binary64_arithmetic_preserves_rounding_zeros_and_nan_semantics() {
    arithmetic(false);
    rounding_and_order(false);
}

#[test]
fn floating_comparisons_do_not_acquire_integer_identities_or_bit_equality() {
    comparisons(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_binary64_arithmetic_and_comparisons() {
    arithmetic(true);
    rounding_and_order(true);
    comparisons(true);
}
