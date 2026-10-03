//! A native u128 rational supplies expectations independently of radix division.

use super::*;

pub(super) fn exact_quotient(
    numerator: u64,
    denominator: u64,
    precision: u32,
    rc: u16,
    negative: bool,
) -> ((u64, u16), u16) {
    let below_one = numerator < denominator;
    let scaled = u128::from(numerator) << (precision - 1 + u32::from(below_one));
    let denominator = u128::from(denominator);
    let integer = scaled / denominator;
    let remainder = scaled % denominator;
    let increment = match rc {
        0 => remainder * 2 > denominator || (remainder * 2 == denominator && integer & 1 != 0),
        1 => remainder != 0 && negative,
        2 => remainder != 0 && !negative,
        _ => false,
    };
    let rounded = integer + u128::from(increment);
    let carry = rounded == 1_u128 << precision;
    let significand = ((rounded >> u32::from(carry)) << (64 - precision)) as u64;
    let exponent = 0x3fff - u16::from(below_one) + u16::from(carry);
    (
        (significand, exponent | if negative { 0x8000 } else { 0 }),
        if remainder != 0 { PE } else { 0 } | if increment { C1 } else { 0 },
    )
}

fn quotients(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let boundaries = [
        LEADING,
        LEADING + 1,
        LEADING + (1 << 11),
        LEADING + (1 << 40),
        0x8000_0000_ffff_ffff,
        0x8000_0001_0000_0000,
        0xbfff_ffff_ffff_ffff,
        0xc000_0000_0000_0000,
        u64::MAX - 1,
        u64::MAX,
    ];
    let mut pairs = Vec::new();
    for numerator in boundaries {
        for denominator in boundaries {
            pairs.push((numerator, denominator));
        }
    }
    // This pair requires both low-digit corrections; the boundary grid above
    // also includes a pair requiring both high-digit corrections.
    pairs.push((0xf8b9_405f_ddd8_e52a, 0x85cd_a95d_e4e5_4775));
    let mut seed = 0x79c8_45a1_932e_f607_u64;
    for _ in 0..96 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let numerator = seed | LEADING;
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        pairs.push((numerator, seed | LEADING));
    }
    for (numerator, denominator) in pairs {
        for (pc, precision) in [(0, 24), (2, 53), (3, 64)] {
            for rc in 0..4 {
                for negative in [false, true] {
                    let (result, flags) =
                        exact_quotient(numerator, denominator, precision, rc, negative);
                    check_arithmetic(
                        &mut checks,
                        [0xde, 0xf9],
                        "exact quotient rounded to PC/RC",
                        Case {
                            left: (numerator, 0x3fff | if negative { 0x8000 } else { 0 }),
                            right: (denominator, 0x3fff),
                            control: 0x007f | (pc << 8) | (rc << 10),
                            result: Some(result),
                            flags,
                        },
                    );
                }
            }
        }
    }
}

fn range_boundaries(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let two = (LEADING, 0x4000);
    let half = (LEADING, 0x3ffe);
    for (numerator, denominator, control, result, flags) in [
        ((LEADING, 1), two, 0x037f, (LEADING >> 1, 0), 0),
        ((LEADING, 1), two, 0x036f, (LEADING, 0x6000), 0x10 | PENDING),
        ((1, 0), two, 0x037f, (0, 0), 0x12 | PE),
        ((1, 0), two, 0x0b7f, (1, 0), 0x12 | PE | C1),
        (
            (u64::MAX, 0x7ffe),
            half,
            0x037f,
            (LEADING, 0x7fff),
            8 | PE | C1,
        ),
        ((u64::MAX, 0x7ffe), half, 0x0f7f, (u64::MAX, 0x7ffe), 8 | PE),
        (
            (u64::MAX, 0x7ffe),
            half,
            0x0377,
            (u64::MAX, 0x1fff),
            8 | PENDING,
        ),
        // Reduced-precision rounding reaches minimum normal before the range
        // test, even though the exact quotient lies just below it.
        (
            (LEADING, 1),
            (LEADING + 1, 0x3fff),
            0x007f,
            (LEADING, 1),
            PE | C1,
        ),
        (
            (LEADING, 1),
            (LEADING + 1, 0x3fff),
            0x037f,
            (LEADING - 1, 0),
            0x10 | PE,
        ),
    ] {
        check_arithmetic(
            &mut checks,
            [0xde, 0xf9],
            "division exponent and subnormal boundaries",
            Case {
                left: numerator,
                right: denominator,
                control,
                result: Some(result),
                flags,
            },
        );
    }
}

test_frontends!(precision, quotients);
test_frontends!(range, range_boundaries);
