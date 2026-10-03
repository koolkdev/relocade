//! Division resolves invalid and zero-divide conditions before denormal operands.

use super::*;

fn operand_responses(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let one = (LEADING, 0x3fff);
    let infinity = (LEADING, 0x7fff);
    let snan = (LEADING | 0x123, 0xffff);
    let qnan = (0xc000_0000_0000_0042, 0x7fff);
    for (numerator, denominator, result, flags) in [
        ((0, 0), one, (0, 0), 0),
        ((0, 0x8000), one, (0, 0x8000), 0),
        ((0, 0x8000), (LEADING, 0xbfff), (0, 0), 0),
        (one, infinity, (0, 0), 0),
        ((LEADING, 0xbfff), infinity, (0, 0x8000), 0),
        (infinity, one, infinity, 0),
        (infinity, (0, 0x8000), (LEADING, 0xffff), 0),
        ((0, 0), (0, 0x8000), INDEFINITE, 1),
        (infinity, (LEADING, 0xffff), INDEFINITE, 1),
        (one, (0, 0x8000), (LEADING, 0xffff), 4),
        (snan, (0, 0), (0xc000_0000_0000_0123, 0xffff), 1),
        (qnan, (0, 0), qnan, 0),
        (one, qnan, qnan, 0),
        (snan, qnan, qnan, 1),
        ((0, 0x3fff), one, INDEFINITE, 1),
        (one, (0, 0x7fff), INDEFINITE, 1),
        (qnan, (1, 0x3fff), INDEFINITE, 1),
        ((1, 0), qnan, qnan, 0),
        ((1, 0), (0, 0), infinity, 4), // #Z suppresses #D, even when DM is clear.
        ((1, 0), one, (1, 0), 2),
        ((LEADING, 0), one, (LEADING, 1), 2),
        ((1, 0), infinity, (0, 0), 2),
        (infinity, (1, 0), infinity, 2),
        ((0, 0x8000), (1, 0), (0, 0x8000), 2),
    ] {
        for reverse in [false, true] {
            check_arithmetic(
                &mut checks,
                [0xde, if reverse { 0xf1 } else { 0xf9 }],
                "division operand classes and priority",
                Case {
                    left: if reverse { denominator } else { numerator },
                    right: if reverse { numerator } else { denominator },
                    control: 0x0357, // Unmask overflow and precision to expose spurious flags.
                    result: Some(result),
                    flags,
                },
            );
        }
    }
    for control in [0x037d, 0x0379] {
        check_arithmetic(
            &mut checks,
            [0xde, 0xf9],
            "zero divide takes priority over an unmasked denormal operand",
            Case {
                left: (1, 0),
                right: (0, 0),
                control,
                result: (control & 4 != 0).then_some(infinity),
                flags: 4 | if control & 4 == 0 { PENDING } else { 0 },
            },
        );
    }
    for (left, right, flag) in [
        (one, (0, 0), 4),
        ((0, 0), (0, 0), 1),
        (infinity, infinity, 1),
        (snan, (1, 0), 1),
        ((1, 0), one, 2),
        (one, (LEADING, 0), 2),
    ] {
        check_arithmetic(
            &mut checks,
            [0xde, 0xf9],
            "unmasked operand exception preserves destination and TOP",
            Case {
                left,
                right,
                control: 0x0340,
                result: None,
                flags: flag | PENDING,
            },
        );
    }
    // Equal NaN payloads retain the original destination's sign in both directions.
    for opcode in [0xf1, 0xf9] {
        check_arithmetic(
            &mut checks,
            [0xde, opcode],
            "reversal preserves NaN destination priority",
            Case {
                left: (qnan.0, 0xffff),
                right: qnan,
                control: 0x037f,
                result: Some((qnan.0, 0xffff)),
                flags: 0,
            },
        );
    }
}

fn empty_operands(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for empty in [0, 7] {
        for masked in [false, true] {
            let code = [0xde, 0xf9];
            let mut image = stack_image(&code, 7, 0x3ffc | (3 << (empty * 2)));
            image.cpu.x87.status.precision = 0;
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x0341 } else { 0x0340 },
            );
            write_register_bits(&mut image.cpu, 0, (1, 0));
            write_register_bits(&mut image.cpu, 7, (0, 0));
            let mut result = complete_x87(image.cpu, 2, 0x06f9);
            result.x87.status = status(if masked { 0x4541 } else { 0xfdc1 });
            if masked {
                write_value(&mut result, 0, INDEFINITE);
                result.x87.tag_word |= 0xc000;
            }
            checks.check(
                "stack fault suppresses zero divide and denormal",
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

test_frontends!(operands, operand_responses);
test_frontends!(stack, empty_operands);
