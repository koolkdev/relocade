//! Numeric conversions observe logical integer widths and Wasm NaN semantics.

use super::*;
use wasm86_compiler::{I16, I8};

fn promotion() {
    let runtime = Fixture::new().expression(&[Type::I32], |body| {
        observed_bits(Val::<F64>::from_binary32_bits(
            body.parameter::<I32>(0).unwrap(),
        ))
    });
    for (source, expected) in [
        (0_u32, 0),
        (0x8000_0000, SIGN),
        (1, 0x36a0_0000_0000_0000),
        (0x007f_ffff, 0x380f_ffff_c000_0000),
        (0x0080_0000, 0x3810_0000_0000_0000),
        (0x3f80_0001, 0x3ff0_0000_2000_0000),
        (0xbf80_0000, 0xbff0_0000_0000_0000),
        (0x7f7f_ffff, 0x47ef_ffff_e000_0000),
        (0x7f80_0000, INFINITY),
        (0xff80_0000, SIGN | INFINITY),
        (0x7fc0_0042, NAN),
        (0xff80_0001, NAN),
    ] {
        let expected = [Value::I64(expected as i64)];
        check(&runtime, &[Value::I32(source as i32)], &expected);
        let constant = Fixture::new().expression(&[], |_| {
            observed_bits(Val::<F64>::from_binary32_bits(source))
        });
        check(&constant, &[], &expected);
    }
}

fn demotion() {
    let observed = |value: Val<F64>| {
        let bits = value.to_binary32_bits();
        bits.and(0x7fff_ffff)
            .unsigned()
            .ge(0x7f80_0001)
            .select(bits.and(0x7fc0_0000), bits)
    };
    let runtime = Fixture::new().expression(&[Type::I64], |body| {
        observed(Val::<F64>::from_bits(body.parameter::<I64>(0).unwrap()))
    });
    // Literal encodings cover ties with even/odd retained bits, gradual
    // underflow, signed zero and rounding across the normal/overflow boundaries.
    for (source, expected) in [
        (0_u64, 0_u32),
        (SIGN, 0x8000_0000),
        (0x3ff0_0000_1000_0000, 0x3f80_0000),
        (0x3ff0_0000_1000_0001, 0x3f80_0001),
        (0x3ff0_0000_3000_0000, 0x3f80_0002),
        (0x3690_0000_0000_0000, 0),
        (0x3690_0000_0000_0001, 1),
        (0xb690_0000_0000_0000, 0x8000_0000),
        (0x36a0_0000_0000_0000, 1),
        (0x380f_ffff_dfff_ffff, 0x007f_ffff),
        (0x380f_ffff_e000_0000, 0x0080_0000),
        (0x47ef_ffff_e000_0000, 0x7f7f_ffff),
        (0x47ef_ffff_f000_0000, 0x7f80_0000),
        (INFINITY, 0x7f80_0000),
        (SIGN | INFINITY, 0xff80_0000),
        (0x7ff8_1234_5678_9abc, 0x7fc0_0000),
        (0xfff0_0000_0000_0001, 0x7fc0_0000),
    ] {
        let expected = [Value::I32(expected as i32)];
        check(&runtime, &[Value::I64(source as i64)], &expected);
        let constant = Fixture::new().expression(&[], |_| observed(Val::<F64>::from_bits(source)));
        check(&constant, &[], &expected);
    }
}

fn narrow_integers() {
    let build = |input: Val<I32>| {
        let bit = input.truncate::<I1>().add(1);
        let byte = input.truncate::<I8>().add(1);
        let word = input.truncate::<I16>().add(1);
        [
            bit.signed().to_f64(),
            bit.unsigned().to_f64(),
            byte.signed().to_f64(),
            byte.unsigned().to_f64(),
            word.signed().to_f64(),
            word.unsigned().to_f64(),
        ]
        .map(|value| value.to_bits())
    };
    let runtime = Fixture::new().function(&[Type::I32], &[Type::I64; 6], |body| {
        let input = body.parameter::<I32>(0)?;
        body.return_(build(input))
    });
    // Narrow addition may leave dirty upper carrier bits. Conversion must
    // observe the wrapped logical value, including I1's signed interpretation.
    for (source, expected) in [
        (0x0000_ffff_u32, [0.0; 6]),
        (0x7fff_ff7f, [0.0, 0.0, -128.0, 128.0, -128.0, 65408.0]),
        (0x1234_7fff, [0.0, 0.0, 0.0, 0.0, -32768.0, 32768.0]),
        (0x8000_0000, [-1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
    ] {
        let expected = expected.map(|value: f64| Value::I64(value.to_bits() as i64));
        check(&runtime, &[Value::I32(source as i32)], &expected);
        for admitted in [false, true] {
            let constant = Fixture::new().function(&[], &[Type::I64; 6], |body| {
                let value = Val::<I32>::from(source);
                let value = if admitted { body.value(value)? } else { value };
                body.return_(build(value))
            });
            check(&constant, &[], &expected);
        }
    }
    let guarded = Fixture::new().function(&[Type::I32], &[Type::I64; 2], |mut body| {
        let byte = body.parameter::<I32>(0)?.truncate::<I8>().add(1);
        let signed = body.value(byte.signed().to_f64())?;
        let unsigned = body.value(byte.unsigned().to_f64())?;
        body.if_(byte.eq(0x80), |arm| {
            arm.return_((signed.to_bits(), unsigned.to_bits()))
        })?;
        body.return_((0_u64, 0_u64))
    });
    check(
        &guarded,
        &[Value::I32(0x1234_007f)],
        &[
            Value::I64(0xc060_0000_0000_0000_u64 as i64),
            Value::I64(0x4060_0000_0000_0000),
        ],
    );
    check(&guarded, &[Value::I32(-1)], &[Value::I64(0), Value::I64(0)]);
}

fn wide_integers() {
    let build = |input: Val<I64>| {
        let word = input.truncate::<I32>();
        [
            word.signed().to_f64(),
            word.unsigned().to_f64(),
            input.signed().to_f64(),
            input.unsigned().to_f64(),
        ]
        .map(|value| value.to_bits())
    };
    let runtime = Fixture::new().function(&[Type::I64], &[Type::I64; 4], |body| {
        let input = body.parameter::<I64>(0)?;
        body.return_(build(input))
    });
    for (source, expected) in [
        (0_u64, [0_u64; 4]),
        (
            0xffff_ffff_ffff_ffff,
            [
                0xbff0_0000_0000_0000,
                0x41ef_ffff_ffe0_0000,
                0xbff0_0000_0000_0000,
                0x43f0_0000_0000_0000,
            ],
        ),
        (
            0x0000_0000_8000_0000,
            [
                0xc1e0_0000_0000_0000,
                0x41e0_0000_0000_0000,
                0x41e0_0000_0000_0000,
                0x41e0_0000_0000_0000,
            ],
        ),
        (
            0x8000_0000_0000_0000,
            [0, 0, 0xc3e0_0000_0000_0000, 0x43e0_0000_0000_0000],
        ),
        (
            0x7fff_ffff_ffff_ffff,
            [
                0xbff0_0000_0000_0000,
                0x41ef_ffff_ffe0_0000,
                0x43e0_0000_0000_0000,
                0x43e0_0000_0000_0000,
            ],
        ),
        (
            0x0020_0000_0000_0001,
            [
                0x3ff0_0000_0000_0000,
                0x3ff0_0000_0000_0000,
                0x4340_0000_0000_0000,
                0x4340_0000_0000_0000,
            ],
        ),
        (
            0x0020_0000_0000_0003,
            [
                0x4008_0000_0000_0000,
                0x4008_0000_0000_0000,
                0x4340_0000_0000_0002,
                0x4340_0000_0000_0002,
            ],
        ),
        (
            0xffdf_ffff_ffff_fffd,
            [
                0xc008_0000_0000_0000,
                0x41ef_ffff_ffa0_0000,
                0xc340_0000_0000_0002,
                0x43ef_fc00_0000_0000,
            ],
        ),
    ] {
        let expected = expected.map(|bits| Value::I64(bits as i64));
        check(&runtime, &[Value::I64(source as i64)], &expected);
        let constant = Fixture::new().function(&[], &[Type::I64; 4], |body| {
            body.return_(build(source.into()))
        });
        check(&constant, &[], &expected);
    }
}

fn saturating_integers() {
    let runtime = Fixture::new().function(&[Type::I64], &[Type::I32], |body| {
        let value = Val::<F64>::from_bits(body.parameter::<I64>(0)?);
        body.return_(value.trunc_saturating_i32())
    });
    for (bits, expected) in [
        (0_u64, 0),
        (SIGN, 0),
        (1, 0),
        (SIGN | 1, 0),
        (1.75_f64.to_bits(), 1),
        ((-1.75_f64).to_bits(), -1),
        (2147483647.0_f64.to_bits(), i32::MAX),
        (2147483647.75_f64.to_bits(), i32::MAX),
        (2147483648.0_f64.to_bits(), i32::MAX),
        ((-2147483648.0_f64).to_bits(), i32::MIN),
        ((-2147483648.75_f64).to_bits(), i32::MIN),
        ((-2147483649.0_f64).to_bits(), i32::MIN),
        (INFINITY, i32::MAX),
        (SIGN | INFINITY, i32::MIN),
        (0x7ff8_0000_0000_0042, 0),
        (0xfff0_0000_0000_0001, 0),
    ] {
        let expected = [Value::I32(expected)];
        check(&runtime, &[Value::I64(bits as i64)], &expected);
        for admitted in [false, true] {
            let constant = Fixture::new().function(&[], &[Type::I32], |body| {
                let value = Val::<F64>::from_bits(bits);
                let value = if admitted { body.value(value)? } else { value };
                body.return_(value.trunc_saturating_i32())
            });
            check(&constant, &[], &expected);
        }
    }
}

#[test]
fn floating_integer_conversion_saturates_without_trapping() {
    saturating_integers();
}

#[test]
fn binary32_promotion_preserves_values_and_defers_nan_payloads() {
    promotion();
}

#[test]
fn binary32_demotion_rounds_ties_and_preserves_gradual_underflow() {
    demotion();
}

#[test]
fn integer_conversions_observe_logical_width_and_nearest_even_rounding() {
    narrow_integers();
    wide_integers();
}
