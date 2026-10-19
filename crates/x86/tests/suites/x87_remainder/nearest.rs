//! Nearest-even quotient boundaries and the sign of an exact IEEE remainder.

use super::*;

const CODE: [u8; 2] = [0xd9, 0xf5];

fn ties_and_signs(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let three_halves = 0xc000_0000_0000_0000;
    for (left, right, output, quotient) in [
        ((three_halves, 0x4000), (LEAD, 0x4000), (LEAD, 0xbfff), 2), // 3 / 2
        (
            (0xa000_0000_0000_0000, 0x4001),
            (LEAD, 0x4000),
            (LEAD, 0x3fff),
            2,
        ), // 5 / 2
        (
            (0xe000_0000_0000_0000, 0x4001),
            (LEAD, 0x4000),
            (LEAD, 0xbfff),
            4,
        ), // 7 / 2
        // An exponent difference of -1 straddles a quotient of one half.
        (
            (three_halves - 1, 0x3ffe),
            (three_halves, 0x3fff),
            (three_halves - 1, 0x3ffe),
            0,
        ),
        (
            (three_halves, 0x3ffe),
            (three_halves, 0x3fff),
            (three_halves, 0x3ffe),
            0,
        ),
        (
            (three_halves + 1, 0x3ffe),
            (three_halves, 0x3fff),
            (three_halves - 1, 0xbffe),
            1,
        ),
        // Carry into quotient bit 3 clears all three published quotient bits.
        (
            (0xf000_0000_0000_0000, 0x4002),
            (LEAD, 0x4000),
            (LEAD, 0xbfff),
            8,
        ), // 15 / 2
    ] {
        for left_sign in [0, 0x8000] {
            for right_sign in [0, 0x8000] {
                let mut image = stack_image(&CODE, 0, 0xffff);
                image.cpu.x87.status.precision = 0;
                write_value(&mut image.cpu, 0, (left.0, left.1 | left_sign));
                write_value(&mut image.cpu, 1, (right.0, right.1 | right_sign));
                let mut result = complete_x87(image.cpu, 2, 0x01f5);
                quotient_flags(&mut result, quotient);
                write_value(&mut result, 0, (output.0, output.1 ^ left_sign));
                checks.check(
                    "nearest-even quotient magnitude and remainder sign",
                    &CODE,
                    &image,
                    &[dispatch(result)],
                );
            }
        }
    }
}

fn exact_integer_reference(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let mut seed = 0x1ac3_072b_f865_e429_u64;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed | LEAD
    };
    for index in 0..128 {
        let numerator = random();
        let denominator = random();
        let difference: i32 = [-2, -1, 0, 1, 31, 32, 52, 53, 62, 63][index % 10];
        // Express both inputs in a common integer unit, then choose the
        // nearest quotient directly with independent u128 division.
        let dividend = u128::from(numerator) << difference.max(0);
        let divisor = u128::from(denominator) << (-difference).max(0);
        let mut quotient = dividend / divisor;
        let mut residual = dividend % divisor;
        let increment = residual * 2 > divisor || (residual * 2 == divisor && quotient & 1 != 0);
        if increment {
            quotient += 1;
            residual = divisor - residual;
        }
        let residual = u64::try_from(residual).unwrap();
        let normalization = residual.leading_zeros();
        let output = if residual == 0 {
            (0, 0)
        } else {
            (
                residual << normalization,
                (0x3fff + difference.min(0) - normalization as i32) as u16
                    | if increment { 0x8000 } else { 0 },
            )
        };
        let mut image = stack_image(&CODE, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, (numerator, (0x3fff + difference) as u16));
        write_value(&mut image.cpu, 1, (denominator, 0x3fff));
        let mut result = complete_x87(image.cpu, 2, 0x01f5);
        quotient_flags(&mut result, (quotient & 7) as u8);
        write_value(&mut result, 0, output);
        checks.check(
            &format!("exact integer reference, exponent gap {difference}"),
            &CODE,
            &image,
            &[dispatch(result)],
        );
    }
}

fn negative_tiny_result(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for unmasked in [false, true] {
        let mut image = stack_image(&CODE, 0, 0xffff);
        set_control(
            &mut image.cpu.x87.control,
            if unmasked { 0x036f } else { 0x037f },
        );
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, (3, 0));
        write_value(&mut image.cpu, 1, (2, 0));
        let mut result = complete_x87(image.cpu, 2, 0x01f5);
        quotient_flags(&mut result, 2);
        result.x87.status.denormal = 1;
        result.x87.status.underflow = u8::from(unmasked);
        result.x87.status.error_summary = u8::from(unmasked);
        result.x87.status.busy = u8::from(unmasked);
        write_value(
            &mut result,
            0,
            if unmasked {
                (LEAD, 0xdfc2)
            } else {
                (1, 0x8000)
            },
        );
        checks.check(
            "negative exact tiny remainder",
            &CODE,
            &image,
            &[dispatch(result)],
        );
    }
}

test_frontends!(nearest_ties, ties_and_signs);
test_frontends!(nearest_reference, exact_integer_reference);
test_frontends!(nearest_tiny, negative_tiny_result);
