//! Conditional moves read both stack operands and leave integer flags untouched.

#[path = "x87_conditional_move/sequences.rs"]
mod sequences;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{
        complete_x87, dispatch, register_bits, set_control, stack_image, write_value, INDEFINITE,
    },
};
use wasm86_x86::SegmentProfile;

const ONE: (u64, u16) = (1 << 63, 0x3fff);
const TWO: (u64, u16) = (1 << 63, 0x4000);

fn initial_image(code: &[u8]) -> Image {
    let mut image = stack_image(code, 7, 0);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.x87.status.c1 = 0x81;
    write_value(&mut image.cpu, 7, ONE);
    write_value(&mut image.cpu, 0, TWO);
    image
}

fn conditions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Truth-table bits are indexed by CF + 2*ZF + 4*PF.
    for (instruction, truth) in [
        ([0xda, 0xc2], 0xaa_u8), // B
        ([0xda, 0xca], 0xcc),    // E
        ([0xda, 0xd2], 0xee),    // BE
        ([0xda, 0xda], 0xf0),    // U
        ([0xdb, 0xc2], 0x55),    // NB
        ([0xdb, 0xca], 0x33),    // NE
        ([0xdb, 0xd2], 0x11),    // NBE
        ([0xdb, 0xda], 0x0f),    // NU
    ] {
        for flags in 0..8 {
            let mut image = initial_image(&instruction);
            image.cpu.flags.bytes.cf = 0x80 | (flags & 1);
            image.cpu.flags.bytes.zf = 0x80 | ((flags >> 1) & 1);
            image.cpu.flags.bytes.pf = 0x80 | ((flags >> 2) & 1);
            write_value(&mut image.cpu, 1, TWO);
            let opcode = (u16::from(instruction[0] & 7) << 8) | u16::from(instruction[1]);
            let mut result = complete_x87(image.cpu, 2, opcode);
            if truth & (1 << flags) != 0 {
                write_value(&mut result, 7, TWO);
            }
            checks.check(
                "all conditions preserve EFLAGS and normal C1",
                &instruction,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

fn raw_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for value in [
        (0, 0x8000),
        (0x8000_0000_0000_0001, 0x7fff), // SNaN
        (0xc000_0000_0000_0042, 0xffff), // QNaN
        (1, 0),                          // denormal
        (0x8000_0000_0000_0042, 0),      // pseudo-denormal
        (1, 0x4000),                     // unsupported
    ] {
        for (source, taken) in [(0, false), (0, true), (1, false), (1, true)] {
            let code = [0xda, 0xc0 | source];
            let mut image = initial_image(&code);
            set_control(&mut image.cpu.x87.control, 0x0c40);
            image.cpu.flags.bytes.cf = u8::from(taken);
            write_value(&mut image.cpu, if source == 0 { 7 } else { 0 }, value);
            let mut result = complete_x87(image.cpu, 2, 0x0200 | u16::from(code[1]));
            if taken {
                write_value(&mut result, 7, value);
            }
            checks.check(
                "raw values copy without numerical exceptions",
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
    let code = [0xda, 0xc1];
    let mut image = initial_image(&code);
    image.cpu.flags.bytes.cf = 0;
    // An untaken move retains an imported tag instead of reclassifying its payload.
    image.cpu.x87.tag_word |= 1 << 14;
    let result = complete_x87(image.cpu, 2, 0x02c1);
    checks.check(
        "untaken move preserves the destination tag",
        &code,
        &image,
        &[dispatch(result)],
    );
}

fn stack_faults(engine: Engine, frontend: Frontend) {
    let code = [0xda, 0xc1, 0x9b];
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for empty in [1, 2, 3] {
        for taken in [false, true] {
            for masked in [false, true] {
                let mut image = initial_image(&code);
                image.cpu.flags.bytes.cf = u8::from(taken);
                set_control(
                    &mut image.cpu.x87.control,
                    if masked { 0x037f } else { 0x037e },
                );
                if empty & 1 != 0 {
                    image.cpu.x87.tag_word |= 0xc000;
                }
                if empty & 2 != 0 {
                    image.cpu.x87.tag_word |= 3;
                }
                let mut result = complete_x87(image.cpu, 2, 0x02c1);
                result.x87.status.invalid = 1;
                result.x87.status.stack_fault = 1;
                result.x87.status.c1 = 0;
                if masked {
                    write_value(&mut result, 7, INDEFINITE);
                } else {
                    result.x87.status.error_summary = 1;
                    result.x87.status.busy = 1;
                }
                let mut waited = result;
                let exit = if masked {
                    waited.eip += 1;
                    waited.instruction_count = waited.instruction_count.wrapping_add(1);
                    Exit::Dispatch(waited.eip)
                } else {
                    Exit::FloatingPoint
                };
                checks.check(
                    "both operands are required even for an untaken move",
                    &code,
                    &image,
                    &[
                        dispatch(result),
                        Step {
                            cpu: waited,
                            ram: &[],
                            exit,
                        },
                    ],
                );
            }
        }
    }
    let mut image = initial_image(&code[..2]);
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    image.cpu.x87.status.invalid = 1;
    checks.check(
        "pending exception preserves all state",
        &code[..2],
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

test_frontends!(forms, conditions);
test_frontends!(payloads, raw_values);
test_frontends!(exceptions, stack_faults);

#[test]
fn complete_encodings() {
    for first in [0xda, 0xdb] {
        for second in [0xc3, 0xcb, 0xd3, 0xdb] {
            crate::support::encoding::check_length(&[first, second]);
        }
    }
    crate::support::encoding::check_length(&[0x66, 0xda, 0xc1]);
}
