//! FRNDINT uses RC, preserves extended integers, and reports operand/precision exceptions.

#[path = "x87_integral/jit.rs"]
mod jit;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Step},
    x87::{complete_x87, dispatch, set_control, stack_image, write_value, INDEFINITE},
};
use wasm86_x86::SegmentProfile;

const LEAD: u64 = 1 << 63;
const CODE: [u8; 2] = [0xd9, 0xfc];

#[test]
fn encoding() {
    check_length(&CODE);
}

fn integral_values(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (input, lower, upper, nearest_up, inexact) in [
        ((0, 0), (0, 0), (0, 0), false, false),
        ((LEAD, 0x3ffd), (0, 0), (LEAD, 0x3fff), false, true), // 1/4
        ((LEAD, 0x3ffe), (0, 0), (LEAD, 0x3fff), false, true), // 1/2
        ((LEAD + 1, 0x3ffe), (0, 0), (LEAD, 0x3fff), true, true),
        (
            (0xc000_0000_0000_0000, 0x3fff),
            (LEAD, 0x3fff),
            (LEAD, 0x4000),
            true,
            true,
        ), // 1.5
        (
            (0xa000_0000_0000_0000, 0x4000),
            (LEAD, 0x4000),
            (0xc000_0000_0000_0000, 0x4000),
            false,
            true,
        ), // 2.5
        (
            (0xc000_0000_0000_0000, 0x4000),
            (0xc000_0000_0000_0000, 0x4000),
            (0xc000_0000_0000_0000, 0x4000),
            false,
            false,
        ),
        (
            (u64::MAX, 0x403d),
            (u64::MAX - 1, 0x403d),
            (LEAD, 0x403e),
            true,
            true,
        ), // 2^63 - 1/2
        (
            (LEAD + 1, 0x403e),
            (LEAD + 1, 0x403e),
            (LEAD + 1, 0x403e),
            false,
            false,
        ),
        (
            (u64::MAX, 0x7ffe),
            (u64::MAX, 0x7ffe),
            (u64::MAX, 0x7ffe),
            false,
            false,
        ),
    ] {
        for sign in [0, 0x8000] {
            for rc in 0..4 {
                // PC has no effect, including on large integers with all 64 bits set.
                for pc in [0, 2, 3] {
                    let mut image = stack_image(&CODE, 7, 0xffff);
                    set_control(&mut image.cpu.x87.control, 0x007f | (pc << 8) | (rc << 10));
                    image.cpu.x87.status.precision = 0;
                    write_value(&mut image.cpu, 7, (input.0, input.1 | sign));
                    let up = inexact
                        && match rc {
                            0 => nearest_up,
                            1 => sign != 0,
                            2 => sign == 0,
                            _ => false,
                        };
                    let magnitude = if up { upper } else { lower };
                    let mut result = complete_x87(image.cpu, 2, 0x01fc);
                    result.x87.status.c1 = u8::from(up);
                    result.x87.status.precision = u8::from(inexact);
                    write_value(&mut result, 7, (magnitude.0, magnitude.1 | sign));
                    checks.check(
                        &format!("{input:x?} sign={sign:x} PC={pc} RC={rc}"),
                        &CODE,
                        &image,
                        &[dispatch(result)],
                    );
                }
            }
        }
    }
}

fn exceptions(frontend: Frontend) {
    let code = [0xd9, 0xfc, 0x9b]; // FRNDINT; FWAIT
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (input, output, flag, inexact, increment) in [
        (Some((LEAD, 0x7fff)), (LEAD, 0x7fff), 0, false, false),
        (Some((LEAD, 0xffff)), (LEAD, 0xffff), 0, false, false),
        (
            Some((0xc000_0000_0000_0042, 0xffff)),
            (0xc000_0000_0000_0042, 0xffff),
            0,
            false,
            false,
        ),
        (
            Some((LEAD + 42, 0x7fff)),
            (0xc000_0000_0000_002a, 0x7fff),
            1,
            false,
            false,
        ),
        (Some((42, 0x3fff)), INDEFINITE, 1, false, false),
        (Some((1, 0x8000)), (0, 0x8000), 2, true, false),
        (Some((LEAD + 1, 0)), (0, 0), 2, true, false),
        (
            Some((0xc000_0000_0000_0000, 0x3fff)),
            (LEAD, 0x4000),
            32,
            true,
            true,
        ),
        (None, INDEFINITE, 1, false, false),
    ] {
        for masked in [false, true] {
            let mut image = stack_image(&code, 2, 0xffff);
            set_control(
                &mut image.cpu.x87.control,
                0x037f & if masked { 0xffff } else { !flag },
            );
            image.cpu.x87.status.precision = 0;
            if let Some(input) = input {
                write_value(&mut image.cpu, 2, input);
            }
            let pending = !masked && flag != 0;
            let suppressed = pending && flag != 32;
            let mut result = complete_x87(image.cpu, 2, 0x01fc);
            result.x87.status.invalid = u8::from(flag == 1);
            result.x87.status.stack_fault = u8::from(input.is_none());
            result.x87.status.denormal = u8::from(flag == 2);
            result.x87.status.precision = u8::from(inexact && !suppressed);
            result.x87.status.c1 = u8::from(increment && !suppressed);
            result.x87.status.error_summary = u8::from(pending);
            result.x87.status.busy = u8::from(pending);
            if !suppressed {
                write_value(&mut result, 2, output);
            }
            let mut waited = result;
            let wait = if pending {
                Step {
                    cpu: waited,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                }
            } else {
                waited.eip += 1;
                waited.instruction_count += 1;
                dispatch(waited)
            };
            checks.check(
                &format!("{input:x?} masked={masked}"),
                &code,
                &image,
                &[dispatch(result), wait],
            );
        }
    }
}

fn entry_faults(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let mut image = stack_image(&CODE, 0, 0xfffc);
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "pending exception keeps metadata and operand",
        &CODE,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

test_frontends!(rounding, integral_values);
test_frontends!(exception_responses, exceptions);
test_frontends!(faults, entry_faults);
