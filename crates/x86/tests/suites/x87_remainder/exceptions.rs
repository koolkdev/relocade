//! Operand exception priority and exact tiny-result responses.

use super::*;
use crate::support::{machine::Step, x87::INDEFINITE};

fn special_operands(&opcode: &u8, engine: Engine, frontend: Frontend) {
    let code = [0xd9, opcode];
    let fop = 0x0100 | u16::from(opcode);
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, right, output, quotient, flags) in [
        ((LEAD, 0x3fff), (0, 0), INDEFINITE, 0, 1),
        ((0, 0), (0, 0), INDEFINITE, 0, 1),
        ((LEAD, 0x7fff), (LEAD, 0x3fff), INDEFINITE, 0, 1),
        ((LEAD, 0xbfff), (LEAD, 0xffff), (LEAD, 0xbfff), 0, 0),
        (
            (LEAD + 1, 0x7fff),
            (LEAD, 0x3fff),
            (0xc000_0000_0000_0001, 0x7fff),
            0,
            1,
        ),
        (
            (0xc000_0000_0000_0042, 0xffff),
            (0, 0),
            (0xc000_0000_0000_0042, 0xffff),
            0,
            0,
        ),
        (
            (1, 0),
            (0xc000_0000_0000_0042, 0x7fff),
            (0xc000_0000_0000_0042, 0x7fff),
            0,
            0,
        ),
        ((1, 0x3fff), (LEAD, 0x3fff), INDEFINITE, 0, 1),
        ((LEAD, 0), (LEAD, 1), (0, 0), 1, 2),
        (
            (3, 0),
            (2, 0),
            (1, if opcode == 0xf5 { 0x8000 } else { 0 }),
            if opcode == 0xf5 { 2 } else { 1 },
            2,
        ),
    ] {
        for masked in [false, true] {
            let mut image = stack_image(&code, 0, 0xffff);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037c },
            );
            image.cpu.x87.status.precision = 0;
            write_value(&mut image.cpu, 0, left);
            write_value(&mut image.cpu, 1, right);
            let suppressed = !masked && flags != 0;
            let mut result = complete_x87(image.cpu, 2, fop);
            result.x87.status.invalid = u8::from(flags == 1);
            result.x87.status.denormal = u8::from(flags == 2);
            result.x87.status.error_summary = u8::from(suppressed);
            result.x87.status.busy = u8::from(suppressed);
            if !suppressed {
                quotient_flags(&mut result, quotient);
                write_value(&mut result, 0, output);
            }
            checks.check(
                &format!("{left:x?} / {right:x?} masked={masked}"),
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
    for empty in [0, 1] {
        for masked in [false, true] {
            let mut image = stack_image(&code, 0, 0xffff);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037e },
            );
            write_value(&mut image.cpu, 1 - empty, (LEAD, 0x3fff));
            let mut result = complete_x87(image.cpu, 2, fop);
            result.x87.status.invalid = 1;
            result.x87.status.stack_fault = 1;
            result.x87.status.c1 = 0;
            result.x87.status.error_summary = u8::from(!masked);
            result.x87.status.busy = u8::from(!masked);
            if masked {
                quotient_flags(&mut result, 0);
                write_value(&mut result, 0, INDEFINITE);
            }
            checks.check(
                "empty operand retains ST1 and TOP",
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

fn underflow(&opcode: &u8, engine: Engine, frontend: Frontend) {
    let code = [0xd9, opcode];
    let fop = 0x0100 | u16::from(opcode);
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for denormal_input in [false, true] {
        for unmasked in [false, true] {
            let mut image = stack_image(&code, 0, 0xffff);
            set_control(
                &mut image.cpu.x87.control,
                if unmasked { 0x036f } else { 0x037f },
            );
            image.cpu.x87.status.precision = 0;
            write_value(
                &mut image.cpu,
                0,
                if denormal_input {
                    (1, 0)
                } else {
                    (LEAD + 1, 1)
                },
            );
            write_value(&mut image.cpu, 1, (LEAD, 1));
            let mut result = complete_x87(image.cpu, 2, fop);
            quotient_flags(&mut result, u8::from(!denormal_input));
            result.x87.status.denormal = u8::from(denormal_input);
            result.x87.status.underflow = u8::from(unmasked);
            result.x87.status.error_summary = u8::from(unmasked);
            result.x87.status.busy = u8::from(unmasked);
            write_value(
                &mut result,
                0,
                if unmasked { (LEAD, 0x5fc2) } else { (1, 0) },
            );
            checks.check(
                "exact tiny result only raises unmasked #U",
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
    let code = [0xd9, opcode, 0x9b];
    let mut image = stack_image(&code, 0, 0xffff);
    set_control(&mut image.cpu.x87.control, 0x036d); // Unmasked #D suppresses #U.
    write_value(&mut image.cpu, 0, (1, 0));
    write_value(&mut image.cpu, 1, (LEAD, 1));
    let mut result = complete_x87(image.cpu, 2, fop);
    result.x87.status.denormal = 1;
    result.x87.status.error_summary = 1;
    result.x87.status.busy = 1;
    checks.check(
        "denormal priority and deferred FWAIT delivery",
        &code,
        &image,
        &[
            dispatch(result),
            Step {
                cpu: result,
                ram: &[],
                exit: Exit::FloatingPoint,
            },
        ],
    );
}

test_frontends!(fprem_specials, 0xf8, special_operands);
test_frontends!(fprem1_specials, 0xf5, special_operands);
test_frontends!(fprem_tiny, 0xf8, underflow);
test_frontends!(fprem1_tiny, 0xf5, underflow);
