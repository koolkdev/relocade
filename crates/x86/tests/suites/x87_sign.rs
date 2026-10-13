//! Sign-only operations preserve raw encodings and use ordinary x87 stack faults.

#[path = "x87_sign/native.rs"]
mod native;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Step},
    step::Engine,
    x87::{complete_x87, dispatch, set_control, stack_image, status, write_value, INDEFINITE},
};
use wasm86_x86::SegmentProfile;

#[test]
fn encodings() {
    for opcode in [0xe0, 0xe1] {
        check_length(&[0xd9, opcode]);
    }
}

fn raw_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for opcode in [0xe0, 0xe1] {
        let code = [0xd9, opcode];
        for (name, significand, exponent) in [
            ("full precision finite", 0x8000_0000_0000_0001, 0x4000),
            ("zero", 0, 0),
            ("denormal", 1, 0),
            ("pseudo-denormal", 0x8000_0000_0000_0042, 0),
            ("infinity", 0x8000_0000_0000_0000, 0x7fff),
            ("quiet NaN", 0xc000_0000_0000_0042, 0x7fff),
            ("signaling NaN", 0x8000_0000_0000_0042, 0x7fff),
            ("unsupported finite", 0x0000_0000_0000_0042, 0x4000),
            ("unsupported special", 0x0000_0000_0000_0042, 0x7fff),
        ] {
            for sign in [0, 0x8000] {
                let mut image = stack_image(&code, 5, 0xffff);
                // Unmasked numerical exceptions and PC24/truncate must not turn
                // a sign change into arithmetic or narrow an extended value.
                set_control(&mut image.cpu.x87.control, 0x0c00);
                write_value(&mut image.cpu, 5, (significand, exponent | sign));
                let mut result = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
                result.x87.status.c1 = 0;
                let result_sign = if opcode == 0xe0 { sign ^ 0x8000 } else { 0 };
                write_value(&mut result, 5, (significand, exponent | result_sign));
                checks.check(
                    &format!("{opcode:x}: {name}, sign={sign:x}"),
                    &code,
                    &image,
                    &[dispatch(result)],
                );
            }
        }
    }
}

fn empty_stack(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for opcode in [0xe0, 0xe1] {
        let code = [0xd9, opcode, 0xdf, 0xe0, 0x9b]; // sign; FNSTSW AX; FWAIT
        for masked in [false, true] {
            let mut image = stack_image(&code, 3, 0xffff);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037e },
            );
            let mut result = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
            let word = if masked { 0x5d61 } else { 0xdde1 };
            result.x87.status = status(word);
            if masked {
                write_value(&mut result, 3, INDEFINITE);
            }
            let mut observed = result;
            observed.eip += 2;
            observed.instruction_count += 1;
            observed.registers.eax = 0x1111_0000 | u32::from(word);
            let waiting = if masked {
                let mut waited = observed;
                waited.eip += 1;
                waited.instruction_count += 1;
                dispatch(waited)
            } else {
                Step {
                    cpu: observed,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                }
            };
            checks.check(
                &format!("{opcode:x}: empty ST0, masked={masked}"),
                &code,
                &image,
                &[dispatch(result), dispatch(observed), waiting],
            );
        }
    }
}

fn pending_exception(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for opcode in [0xe0, 0xe1] {
        let code = [0xd9, opcode];
        let mut image = stack_image(&code, 0, 0xfffc);
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.cpu.x87.status = status(0xc7e1);
        checks.check(
            "pending exception prevents sign and metadata updates",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::FloatingPoint,
            }],
        );
    }
}

fn continued_sign_changes(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0xd9, 0xe0, 0xd9, 0xe1, 0xd9, 0xe0, 0xa1, 0, 0x50, 0, 0];
    for empty in [false, true] {
        let mut image = stack_image(&code, 0, 0xffff);
        if !empty {
            write_value(&mut image.cpu, 0, (0, 0));
        }
        let mut negated = complete_x87(image.cpu, 2, 0x01e0);
        negated.x87.status.c1 = 0;
        let significand = if empty { INDEFINITE.0 } else { 0 };
        let exponent = if empty { 0x7fff } else { 0 };
        if empty {
            negated.x87.status.invalid = 1;
            negated.x87.status.stack_fault = 1;
        }
        write_value(&mut negated, 0, (significand, exponent | 0x8000));
        let mut absolute = complete_x87(negated, 2, 0x01e1);
        write_value(&mut absolute, 0, (significand, exponent));
        let mut negative = complete_x87(absolute, 2, 0x01e0);
        write_value(&mut negative, 0, (significand, exponent | 0x8000));
        checks.check(
            "successive sign changes survive a later memory fault",
            &code,
            &image,
            &[
                dispatch(negated),
                dispatch(absolute),
                dispatch(negative),
                Step {
                    cpu: negative,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x5000,
                        error: 0,
                    },
                },
            ],
        );
    }
}

test_frontends!(encodings_and_values, raw_values);
test_frontends!(stack_underflow, empty_stack);
test_frontends!(pending, pending_exception);
test_frontends!(continuations, continued_sign_changes);
