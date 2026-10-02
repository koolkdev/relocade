//! Exact host products supply independent quotient/remainder rounding expectations.

use super::*;

fn precision_and_rounding(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, right) in [
        (LEADING, LEADING),
        (LEADING + 1, LEADING + 1),
        (LEADING + 1, 0xc000_0000_0000_0000),
        (LEADING + 3, 0xc000_0000_0000_0000),
        (LEADING + (1 << 32), LEADING + (1 << 31)),
        (LEADING + (1 << 32), LEADING + (1 << 31) + 1),
        (LEADING + (1 << 32), LEADING + (1 << 31) - 1),
        (0xc000_0000_0000_0000, 0xaaaa_aaaa_aaaa_aaaa),
        (0xc000_0000_0000_0000, 0xaaaa_aaaa_aaaa_aaab),
        (u64::MAX, LEADING),
        (u64::MAX, u64::MAX),
    ] {
        let product = u128::from(left) * u128::from(right);
        let bits = 128 - product.leading_zeros();
        for (pc, precision) in [(0, 24), (2, 53), (3, 64)] {
            let divisor = 2_u128.pow(bits - precision);
            let quotient = product / divisor;
            let remainder = product % divisor;
            for negative in [false, true] {
                for rc in 0..4 {
                    let increment = match rc {
                        0 => {
                            remainder > divisor / 2
                                || (remainder == divisor / 2 && quotient % 2 != 0)
                        }
                        1 => remainder != 0 && negative,
                        2 => remainder != 0 && !negative,
                        _ => false,
                    };
                    let rounded = quotient + u128::from(increment);
                    let carry = rounded == 2_u128.pow(precision);
                    let significand = ((rounded >> u32::from(carry)) << (64 - precision)) as u64;
                    let exponent = 0x3fff + (bits - 127) as u16 + u16::from(carry);
                    check_product(
                        &mut checks,
                        "exact product rounded to PC/RC",
                        Case {
                            left: (left, if negative { 0xbfff } else { 0x3fff }),
                            right: (right, 0x3fff),
                            control: 0x007f | (pc << 8) | (rc << 10),
                            result: Some((
                                significand,
                                exponent | if negative { 0x8000 } else { 0 },
                            )),
                            flags: if remainder != 0 { PE } else { 0 }
                                | if increment { C1 } else { 0 },
                        },
                    );
                }
            }
        }
    }
}

fn exponent_range(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (pc, precision) in [(0, 24), (2, 53), (3, 64)] {
        for negative in [false, true] {
            let sign = if negative { 0x8000 } else { 0 };
            for rc in 0..4 {
                let infinity = rc == 0 || (rc == 1 && negative) || (rc == 2 && !negative);
                for masked in [false, true] {
                    let (result, flags) = if !masked {
                        // 2^16384 adjusted by 2^-24576 is exactly 2^-8192.
                        ((LEADING, 0x1fff | sign), 8 | PENDING)
                    } else if infinity {
                        ((LEADING, 0x7fff | sign), 8 | PE | C1)
                    } else {
                        ((u64::MAX << (64 - precision), 0x7ffe | sign), 8 | PE)
                    };
                    check_product(
                        &mut checks,
                        "overflow response",
                        Case {
                            left: (LEADING, 0x7ffe | sign),
                            right: (LEADING, 0x4000),
                            control: 0x0077 | (pc << 8) | (rc << 10) | if masked { 8 } else { 0 },
                            result: Some(result),
                            flags,
                        },
                    );
                }
            }
        }
        // Exact tiny results only raise UE when it is unmasked. Reduced PC
        // retains the extended exponent range, not the binary32/64 range.
        for masked in [false, true] {
            check_product(
                &mut checks,
                "exact tiny product",
                Case {
                    left: (LEADING, 1),
                    right: (LEADING, 0x3ffe),
                    control: 0x006f | (pc << 8) | if masked { 0x10 } else { 0 },
                    result: Some(if masked {
                        (LEADING >> 1, 0)
                    } else {
                        (LEADING, 0x6000)
                    }),
                    flags: if masked { 0 } else { 0x10 | PENDING },
                },
            );
        }
        let unit = 1_u64 << (64 - precision);
        // Rounding on the subnormal grid must use the original product. The
        // exact result lies just above the midpoint between 0 and one unit;
        // precision rounding first would turn it into an even tie at zero.
        for rc in 0..4 {
            let increment = rc == 0 || rc == 2;
            check_product(
                &mut checks,
                "direct subnormal rounding avoids a double round",
                Case {
                    left: (LEADING + 1, 1),
                    right: (u64::MAX, 0x3ffe - precision as u16),
                    control: 0x007f | (pc << 8) | (rc << 10),
                    result: Some((if increment { unit } else { 0 }, 0)),
                    flags: 0x10 | PE | if increment { C1 } else { 0 },
                },
            );
        }
    }
    for (left, right, result, flags) in [
        // Precision rounding reaches the smallest normal: tininess is tested
        // after that rounding, so this inexact result does not raise UE.
        (
            (LEADING + 1, 1),
            (u64::MAX - 1, 0x3ffe),
            (LEADING, 1),
            PE | C1,
        ),
        // Subnormal rounding reaches normal only after the tininess test.
        (
            (u64::MAX, 1),
            (LEADING, 0x3ffe),
            (LEADING, 1),
            0x10 | PE | C1,
        ),
        // A full-width subnormal result that is exactly representable.
        (
            (LEADING + 2, 1),
            (LEADING, 0x3ffe),
            ((LEADING >> 1) + 1, 0),
            0,
        ),
        // The smallest possible product stays tiny without a wrapped shift.
        ((1, 0), (1, 0), (0, 0), 2 | 0x10 | PE),
    ] {
        check_product(
            &mut checks,
            "underflow boundaries",
            Case {
                left,
                right,
                control: 0x037f,
                result: Some(result),
                flags,
            },
        );
    }
    check_product(
        &mut checks,
        "unmasked tiny response retains precision evidence before denormalization",
        Case {
            left: (u64::MAX, 1),
            right: (LEADING, 0x3ffe),
            control: 0x036f,
            result: Some((u64::MAX, 0x6000)),
            flags: 0x10 | PENDING,
        },
    );
    check_product(
        &mut checks,
        "adjusted overflow retains its rounding increment",
        Case {
            left: (LEADING + 1, 0x7ffe),
            right: (0xc000_0000_0000_0000, 0x4000),
            control: 0x0377,
            result: Some((0xc000_0000_0000_0002, 0x1fff)),
            flags: 8 | PE | C1 | PENDING,
        },
    );
}

test_frontends!(precision, precision_and_rounding);
test_frontends!(range, exponent_range);
