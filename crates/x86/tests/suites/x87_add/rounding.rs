//! An exact integer numerator supplies expectations independently of alignment.

use super::*;

pub(super) fn exact_sum(
    left: u64,
    right: u64,
    gap: u32,
    negative_left: bool,
    negative_right: bool,
    precision: u32,
    rc: u16,
) -> ((u64, u16), u16) {
    let a = u128::from(left) << gap;
    let b = u128::from(right);
    let (numerator, negative) = if negative_left == negative_right {
        (a + b, negative_left)
    } else if a >= b {
        (a - b, negative_left)
    } else {
        (b - a, negative_right)
    };
    if numerator == 0 {
        return ((0, if rc == 1 { 0x8000 } else { 0 }), 0);
    }
    let bits = 128 - numerator.leading_zeros();
    let (integer, remainder, divisor) = if bits > precision {
        let divisor = 1_u128 << (bits - precision);
        (numerator / divisor, numerator % divisor, divisor)
    } else {
        (numerator << (precision - bits), 0, 1)
    };
    let increment = match rc {
        0 => {
            remainder > divisor / 2
                || (remainder != 0 && remainder == divisor / 2 && integer & 1 != 0)
        }
        1 => remainder != 0 && negative,
        2 => remainder != 0 && !negative,
        _ => false,
    };
    let rounded = integer + u128::from(increment);
    let carry = rounded == 1_u128 << precision;
    let significand = ((rounded >> u32::from(carry)) << (64 - precision)) as u64;
    let exponent = (0x3fff_i32 + bits as i32 - 64 - gap as i32 + i32::from(carry)) as u16;
    (
        (significand, exponent | if negative { 0x8000 } else { 0 }),
        if remainder != 0 { PE } else { 0 } | if increment { C1 } else { 0 },
    )
}

fn precision_and_cancellation(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, right) in [
        (LEADING, LEADING),
        (LEADING, u64::MAX),
        (LEADING + 1, LEADING + 3),
        (u64::MAX, LEADING),
        (u64::MAX, u64::MAX),
        (0xc000_0000_0000_0001, 0xaaaa_aaaa_aaaa_aaab),
    ] {
        for gap in [0, 1, 2, 10, 11, 12, 39, 40, 41, 62, 63] {
            for (pc, precision) in [(0, 24), (2, 53), (3, 64)] {
                for negative in [false, true] {
                    for opposite in [false, true] {
                        for rc in 0..4 {
                            for subtract in [false, true] {
                                let (bits, flags) = exact_sum(
                                    left,
                                    right,
                                    gap,
                                    negative,
                                    negative ^ opposite ^ subtract,
                                    precision,
                                    rc,
                                );
                                check_arithmetic(
                                    &mut checks,
                                    [0xde, if subtract { 0xe9 } else { 0xc1 }],
                                    "exact numerator rounded to PC/RC",
                                    Case {
                                        left: (left, 0x3fff | if negative { 0x8000 } else { 0 }),
                                        right: (
                                            right,
                                            (0x3fff - gap as u16)
                                                | if negative ^ opposite { 0x8000 } else { 0 },
                                        ),
                                        control: 0x007f | (pc << 8) | (rc << 10),
                                        result: Some(bits),
                                        flags,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn wide_gaps(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // These expectations are distances from 1 on the two adjacent binades.
    // In particular, subtraction at gap 65 can land on either side of the
    // midpoint below 1; blindly treating every large gap as sticky loses it.
    for (gap, smaller, subtract, nearest, nearest_c1, exact) in [
        (64, LEADING, false, (LEADING, 0x3fff), false, false),
        (64, LEADING + 1, false, (LEADING + 1, 0x3fff), true, false),
        (64, LEADING, true, (u64::MAX, 0x3ffe), false, true),
        (64, LEADING + 1, true, (u64::MAX, 0x3ffe), true, false),
        (64, u64::MAX, true, (u64::MAX - 1, 0x3ffe), false, false),
        (65, LEADING, true, (LEADING, 0x3fff), true, false),
        (65, u64::MAX, true, (u64::MAX, 0x3ffe), false, false),
        (66, u64::MAX, true, (LEADING, 0x3fff), true, false),
        (127, u64::MAX, true, (LEADING, 0x3fff), true, false),
        (128, u64::MAX, true, (LEADING, 0x3fff), true, false),
        (200, u64::MAX, false, (LEADING, 0x3fff), false, false),
        (16000, u64::MAX, true, (LEADING, 0x3fff), true, false),
    ] {
        check_arithmetic(
            &mut checks,
            [0xde, if subtract { 0xe9 } else { 0xc1 }],
            "word and distant alignment boundaries",
            Case {
                left: (LEADING, 0x3fff),
                right: (smaller, 0x3fff - gap),
                control: 0x037f,
                result: Some(nearest),
                flags: if exact { 0 } else { PE } | if nearest_c1 { C1 } else { 0 },
            },
        );
    }
    for gap in [66, 127, 128, 16000] {
        for negative in [false, true] {
            for subtract in [false, true] {
                for rc in 1..4 {
                    let away = (rc == 1 && negative) || (rc == 2 && !negative);
                    let bits = if subtract {
                        if away {
                            (LEADING, 0x3fff)
                        } else {
                            (u64::MAX, 0x3ffe)
                        }
                    } else {
                        (LEADING + u64::from(away), 0x3fff)
                    };
                    let sign = if negative { 0x8000 } else { 0 };
                    check_arithmetic(
                        &mut checks,
                        [0xde, if subtract { 0xe9 } else { 0xc1 }],
                        "directed rounding of a distant operand",
                        Case {
                            left: (LEADING, 0x3fff | sign),
                            right: (u64::MAX, (0x3fff - gap) | sign),
                            control: 0x037f | (rc << 10),
                            result: Some((bits.0, bits.1 | sign)),
                            flags: PE | if away { C1 } else { 0 },
                        },
                    );
                }
            }
        }
    }
}

test_frontends!(precision, precision_and_cancellation);
test_frontends!(gaps, wide_gaps);
