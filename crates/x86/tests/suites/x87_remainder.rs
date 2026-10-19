//! Exact remainders, partial reduction, quotient flags, and operand exceptions.

#[path = "x87_remainder/exceptions.rs"]
mod exceptions;
#[path = "x87_remainder/jit.rs"]
mod jit;
#[path = "x87_remainder/nearest.rs"]
mod nearest;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::Exit,
    x87::{complete_x87, dispatch, set_control, stack_image, write_value},
};
use relocade_x86::{CpuState, SegmentProfile};

const LEAD: u64 = 1 << 63;

fn quotient_flags(cpu: &mut CpuState, quotient: u8) {
    cpu.x87.status.c0 = (quotient >> 2) & 1;
    cpu.x87.status.c3 = (quotient >> 1) & 1;
    cpu.x87.status.c1 = quotient & 1;
    cpu.x87.status.c2 = 0;
}

#[test]
fn encoding() {
    check_length(&[0xd9, 0xf8]);
    check_length(&[0xd9, 0xf5]);
}

fn finite_values(&opcode: &u8, frontend: Frontend) {
    let code = [0xd9, opcode];
    let fop = 0x0100 | u16::from(opcode);
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // Dividing 2*q + 0.5 by 2 exercises every quotient condition-code pattern.
    let dividends = [
        (LEAD, 0x3ffe),
        (0xa000_0000_0000_0000, 0x4000),
        (0x9000_0000_0000_0000, 0x4001),
        (0xd000_0000_0000_0000, 0x4001),
        (0x8800_0000_0000_0000, 0x4002),
        (0xa800_0000_0000_0000, 0x4002),
        (0xc800_0000_0000_0000, 0x4002),
        (0xe800_0000_0000_0000, 0x4002),
    ];
    for (quotient, dividend) in dividends.into_iter().enumerate() {
        for left_sign in [0, 0x8000] {
            for right_sign in [0, 0x8000] {
                let mut image = stack_image(&code, 7, 0xffff);
                image.cpu.x87.status.precision = 0;
                write_value(&mut image.cpu, 7, (dividend.0, dividend.1 | left_sign));
                write_value(&mut image.cpu, 0, (LEAD, 0x4000 | right_sign));
                let mut result = complete_x87(image.cpu, 2, fop);
                quotient_flags(&mut result, quotient as u8);
                write_value(&mut result, 7, (LEAD, 0x3ffe | left_sign));
                checks.check(
                    &format!("quotient magnitude={quotient} signs={left_sign:x}/{right_sign:x}"),
                    &code,
                    &image,
                    &[dispatch(result)],
                );
            }
        }
    }
    for (left, right, output, quotient) in [
        ((0, 0x8000), (LEAD, 0x3fff), (0, 0x8000), 0),
        (
            (0xc000_0000_0000_0000, 0xc001),
            (0xc000_0000_0000_0000, 0x4000),
            (0, 0x8000),
            2,
        ),
        (
            (LEAD, 0x403e),
            (0xc000_0000_0000_0000, 0x3fff),
            (LEAD, 0x3ffe),
            5,
        ),
        (
            (u64::MAX, 0x403e),
            (LEAD + 1, 0x3fff),
            (0xc000_0000_0000_0000, 0x3fc1),
            5,
        ),
    ] {
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, left);
        write_value(&mut image.cpu, 1, right);
        let mut result = complete_x87(image.cpu, 2, fop);
        quotient_flags(&mut result, quotient);
        write_value(&mut result, 0, output);
        checks.check(
            "signed zero and 64-bit quotient boundary",
            &code,
            &image,
            &[dispatch(result)],
        );
    }
}

fn controls(&opcode: &u8, frontend: Frontend) {
    let code = [0xd9, opcode];
    let fop = 0x0100 | u16::from(opcode);
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for pc in 0..4 {
        for rc in 0..4 {
            let mut image = stack_image(&code, 0, 0xffff);
            set_control(&mut image.cpu.x87.control, 0x005f | (pc << 8) | (rc << 10));
            image.cpu.x87.status.precision = 0;
            write_value(&mut image.cpu, 0, (LEAD + 3, 0x4000));
            write_value(&mut image.cpu, 1, (LEAD, 0x3fff));
            let mut result = complete_x87(image.cpu, 2, fop);
            quotient_flags(&mut result, 2);
            write_value(&mut result, 0, (0xc000_0000_0000_0000, 0x3fc2));
            checks.check(
                &format!("PC={pc} RC={rc} never rounds or raises #P"),
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

fn partial_reductions(&opcode: &u8, frontend: Frontend) {
    let code = [0xd9, opcode];
    let fop = 0x0100 | u16::from(opcode);
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = code.repeat(3);
    let mut image = stack_image(&code, 0, 0xffff);
    write_value(&mut image.cpu, 0, (LEAD, 0x405f)); // 2^96
    write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x3fff)); // 1.5
    let mut result = image.cpu;
    let mut steps = Vec::new();
    let last_exponent = if opcode == 0xf5 { 0xbffe } else { 0x3fff };
    for exponent in [0x403f, 0x401f, last_exponent] {
        result = complete_x87(result, 2, fop);
        result.x87.status.c2 = u8::from(exponent != last_exponent);
        if exponent == last_exponent {
            quotient_flags(&mut result, if opcode == 0xf5 { 3 } else { 2 });
        }
        write_value(&mut result, 0, (LEAD, exponent));
        steps.push(dispatch(result));
    }
    checks.check(
        "partial reductions make progress and eventually clear C2",
        &code,
        &image,
        &steps,
    );

    let code = [0xd9, opcode].repeat(2);
    let mut image = stack_image(&code, 0, 0xffff);
    write_value(&mut image.cpu, 0, (LEAD, 0xffff - 1));
    write_value(&mut image.cpu, 1, (LEAD, 1));
    let mut partial = complete_x87(image.cpu, 2, fop);
    partial.x87.status.c2 = 1;
    write_value(&mut partial, 0, (0, 0x8000));
    let mut done = complete_x87(partial, 2, fop);
    quotient_flags(&mut done, 0);
    checks.check(
        "maximum exponent gap and zero partial remainder",
        &code,
        &image,
        &[dispatch(partial), dispatch(done)],
    );
}

test_frontends!(fprem_finite, 0xf8, finite_values);
test_frontends!(fprem1_finite, 0xf5, finite_values);
test_frontends!(fprem_controls, 0xf8, controls);
test_frontends!(fprem1_controls, 0xf5, controls);
test_frontends!(fprem_partial, 0xf8, partial_reductions);
test_frontends!(fprem1_partial, 0xf5, partial_reductions);
