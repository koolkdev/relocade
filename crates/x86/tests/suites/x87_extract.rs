//! FXTRACT preserves exact significands and applies one response to both output slots.

#[path = "x87_extract/jit.rs"]
mod jit;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Step},
    step::Engine,
    x87::{complete_x87, dispatch, set_control, stack_image, write_value, INDEFINITE},
};
use wasm86_x86::SegmentProfile;

const LEAD: u64 = 1 << 63;
const CODE: [u8; 2] = [0xd9, 0xf4];

#[test]
fn encoding() {
    check_length(&CODE);
}

fn values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (input, significand, exponent, flags) in [
        ((LEAD + 1, 0x4000), (LEAD + 1, 0x3fff), (LEAD, 0x3fff), 0),
        (
            (0xc000_0000_0000_0000, 0xbfff),
            (0xc000_0000_0000_0000, 0xbfff),
            (0, 0),
            0,
        ),
        ((LEAD, 0x3ffe), (LEAD, 0x3fff), (LEAD, 0xbfff), 0),
        (
            (u64::MAX, 0x7ffe),
            (u64::MAX, 0x3fff),
            (0xfffc_0000_0000_0000, 0x400c),
            0,
        ),
        (
            (LEAD, 1),
            (LEAD, 0x3fff),
            (0xfff8_0000_0000_0000, 0xc00c),
            0,
        ),
        ((1, 0), (LEAD, 0x3fff), (0x807a_0000_0000_0000, 0xc00d), 2),
        (
            (LEAD + 1, 0x8000),
            (LEAD + 1, 0xbfff),
            (0xfff8_0000_0000_0000, 0xc00c),
            2,
        ),
        ((0, 0), (0, 0), (LEAD, 0xffff), 4),
        ((0, 0x8000), (0, 0x8000), (LEAD, 0xffff), 4),
        ((LEAD, 0x7fff), (LEAD, 0x7fff), (LEAD, 0x7fff), 0),
        ((LEAD, 0xffff), (LEAD, 0xffff), (LEAD, 0x7fff), 0),
        (
            (0xc000_0000_0000_0042, 0xffff),
            (0xc000_0000_0000_0042, 0xffff),
            (0xc000_0000_0000_0042, 0xffff),
            0,
        ),
        (
            (LEAD + 42, 0x7fff),
            (0xc000_0000_0000_002a, 0x7fff),
            (0xc000_0000_0000_002a, 0x7fff),
            1,
        ),
        ((42, 0x3fff), INDEFINITE, INDEFINITE, 1),
    ] {
        for control in [0x037f, 0x087f, 0x0378] {
            let mut image = stack_image(&CODE, 0, 0xffff);
            set_control(&mut image.cpu.x87.control, control);
            write_value(&mut image.cpu, 0, input);
            let suppressed = flags & !control != 0;
            let mut result = complete_x87(image.cpu, 2, 0x01f4);
            result.x87.status.c1 = 0;
            result.x87.status.invalid = u8::from(flags == 1);
            result.x87.status.denormal = u8::from(flags == 2);
            result.x87.status.zero_divide = u8::from(flags == 4);
            result.x87.status.error_summary = u8::from(suppressed);
            result.x87.status.busy = u8::from(suppressed);
            if !suppressed {
                result.x87.status.top = 7;
                write_value(&mut result, 0, exponent);
                write_value(&mut result, 7, significand);
            }
            checks.check(
                &format!("{input:x?} CW={control:x}"),
                &CODE,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

fn stack_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for empty in [false, true] {
        for full in [false, true] {
            if !empty && !full {
                continue;
            }
            for masked in [false, true] {
                let code = [0xd9, 0xf4, 0x9b];
                let mut image = stack_image(&code, 0, 0xffff);
                set_control(
                    &mut image.cpu.x87.control,
                    if masked { 0x037f } else { 0x037e },
                );
                if !empty {
                    // The stack fault precedes #Z from the zero operand.
                    write_value(&mut image.cpu, 0, (0, 0));
                }
                if full {
                    write_value(&mut image.cpu, 7, (LEAD, 0x3fff));
                }
                let mut result = complete_x87(image.cpu, 2, 0x01f4);
                result.x87.status.invalid = 1;
                result.x87.status.stack_fault = 1;
                result.x87.status.c1 = u8::from(full && !empty);
                result.x87.status.error_summary = u8::from(!masked);
                result.x87.status.busy = u8::from(!masked);
                if masked {
                    write_value(&mut result, 0, INDEFINITE);
                    write_value(&mut result, 7, INDEFINITE);
                    result.x87.status.top = 7;
                }
                let waited = if masked {
                    let mut wait = result;
                    wait.eip += 1;
                    wait.instruction_count += 1;
                    dispatch(wait)
                } else {
                    Step {
                        cpu: result,
                        ram: &[],
                        exit: Exit::FloatingPoint,
                    }
                };
                checks.check(
                    &format!("empty={empty} full={full} masked={masked}"),
                    &code,
                    &image,
                    &[dispatch(result), waited],
                );
            }
        }
    }
}

fn inverse_scale(engine: Engine, frontend: Frontend) {
    let code = [0xd9, 0xf4, 0xd9, 0xfd, 0xdd, 0xd9]; // FXTRACT; FSCALE; FSTP ST1
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (input, exponent) in [
        ((LEAD + 42, 0x4001), (LEAD, 0x4000)),
        ((u64::MAX, 0xbfff), (0, 0)),
        ((1, 0), (0x807a_0000_0000_0000, 0xc00d)),
    ] {
        let mut image = stack_image(&code, 0, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x007f); // Must retain all 64 input bits at PC24.
        write_value(&mut image.cpu, 0, input);
        let mut extracted = complete_x87(image.cpu, 2, 0x01f4);
        extracted.x87.status.top = 7;
        extracted.x87.status.c1 = 0;
        extracted.x87.status.denormal = u8::from(input.1 == 0);
        write_value(&mut extracted, 0, exponent);
        write_value(
            &mut extracted,
            7,
            (
                if input.1 == 0 { LEAD } else { input.0 },
                0x3fff | (input.1 & 0x8000),
            ),
        );
        let mut scaled = complete_x87(extracted, 2, 0x01fd);
        write_value(&mut scaled, 7, input);
        let mut restored = complete_x87(scaled, 2, 0x05d9);
        write_value(&mut restored, 0, input);
        restored.x87.status.top = 0;
        restored.x87.tag_word |= 0xc000;
        checks.check(
            "extract then scale restores the exact original value",
            &code,
            &image,
            &[dispatch(extracted), dispatch(scaled), dispatch(restored)],
        );
    }
}

test_frontends!(operand_responses, values);
test_frontends!(stack_responses, stack_faults);
test_frontends!(inverse, inverse_scale);
