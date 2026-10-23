//! Integral rounding and square root agree for runtime and folded operands.

use super::*;

fn operations(value: Val<F64>) -> [Val<I64>; 5] {
    [
        value.ceil(),
        value.floor(),
        value.trunc(),
        value.nearest(),
        value.sqrt(),
    ]
    .map(observed_bits)
}

#[test]
fn integral_rounding_and_square_root_preserve_ieee_boundaries() {
    let runtime = Fixture::new().function(&[Type::I64], &[Type::I64; 5], move |body| {
        let value = Val::<F64>::from_bits(body.parameter::<I64>(0)?);
        body.return_(operations(value))
    });
    // Literal encodings include ties, subnormals and exact/irrational roots.
    for (input, expected) in [
        (0, [0; 5]),
        (SIGN, [SIGN; 5]),
        (INFINITY, [INFINITY; 5]),
        (
            SIGN | INFINITY,
            [
                SIGN | INFINITY,
                SIGN | INFINITY,
                SIGN | INFINITY,
                SIGN | INFINITY,
                NAN,
            ],
        ),
        (NAN | 42, [NAN; 5]),
        (SIGN | INFINITY | 1, [NAN; 5]),
        (1, [0x3ff0_0000_0000_0000, 0, 0, 0, 0x1e60_0000_0000_0000]),
        (SIGN | 1, [SIGN, 0xbff0_0000_0000_0000, SIGN, SIGN, NAN]),
        (
            0x3fe0_0000_0000_0000,
            [0x3ff0_0000_0000_0000, 0, 0, 0, 0x3fe6_a09e_667f_3bcd],
        ),
        (
            0xbfe0_0000_0000_0000,
            [SIGN, 0xbff0_0000_0000_0000, SIGN, SIGN, NAN],
        ),
        (
            0x3ff8_0000_0000_0000,
            [
                0x4000_0000_0000_0000,
                0x3ff0_0000_0000_0000,
                0x3ff0_0000_0000_0000,
                0x4000_0000_0000_0000,
                0x3ff3_988e_1409_212e,
            ],
        ),
        (
            0xbff8_0000_0000_0000,
            [
                0xbff0_0000_0000_0000,
                0xc000_0000_0000_0000,
                0xbff0_0000_0000_0000,
                0xc000_0000_0000_0000,
                NAN,
            ],
        ),
        (
            0x4004_0000_0000_0000,
            [
                0x4008_0000_0000_0000,
                0x4000_0000_0000_0000,
                0x4000_0000_0000_0000,
                0x4000_0000_0000_0000,
                0x3ff9_4c58_3ada_5b53,
            ],
        ),
        (
            0xc004_0000_0000_0000,
            [
                0xc000_0000_0000_0000,
                0xc008_0000_0000_0000,
                0xc000_0000_0000_0000,
                0xc000_0000_0000_0000,
                NAN,
            ],
        ),
        (
            0x4010_0000_0000_0000,
            [
                0x4010_0000_0000_0000,
                0x4010_0000_0000_0000,
                0x4010_0000_0000_0000,
                0x4010_0000_0000_0000,
                0x4000_0000_0000_0000,
            ],
        ),
        (
            0x4340_0000_0000_0001,
            [
                0x4340_0000_0000_0001,
                0x4340_0000_0000_0001,
                0x4340_0000_0000_0001,
                0x4340_0000_0000_0001,
                0x4196_a09e_667f_3bcd,
            ],
        ),
    ] {
        let expected = expected.map(|bits| Value::I64(bits as i64));
        check(&runtime, &[Value::I64(input as i64)], &expected);
        for admitted in [false, true] {
            let constant = Fixture::new().function(&[], &[Type::I64; 5], move |body| {
                let value = Val::<F64>::from_bits(input);
                let value = if admitted { body.value(value)? } else { value };
                body.return_(operations(value))
            });
            check(&constant, &[], &expected);
        }
    }
}
