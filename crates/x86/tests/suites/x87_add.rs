//! Addition and subtraction preserve operand order, rounding and stack effects.

#[path = "x87_add/exceptions.rs"]
mod exceptions;
#[path = "x87_add/native.rs"]
mod native;
#[path = "x87_add/rounding.rs"]
mod rounding;
#[path = "x87_add/specialization.rs"]
mod specialization;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Step},
    step::Engine,
    x87::{
        arithmetic::{check_arithmetic, ArithmeticCase as Case},
        complete_x87, dispatch, stack_image, write_value, INDEFINITE,
    },
};
use wasm86_x86::SegmentProfile;

const LEADING: u64 = 1 << 63;
const PE: u16 = 0x20;
const C1: u16 = 0x200;
const PENDING: u16 = 0x8080;

fn register_forms(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // ST0=-2 and ST2=3 expose both subtraction directions. TOP=7 also checks
    // that the destination uses the old logical index across a wrapping pop.
    for (prefix, base, destination, pop, result) in [
        (0xd8, 0xc0, 7, false, (LEADING, 0x3fff)),
        (0xdc, 0xc0, 1, false, (LEADING, 0x3fff)),
        (0xde, 0xc0, 1, true, (LEADING, 0x3fff)),
        (0xd8, 0xe0, 7, false, (0xa000_0000_0000_0000, 0xc001)),
        (0xdc, 0xe8, 1, false, (0xa000_0000_0000_0000, 0x4001)),
        (0xde, 0xe8, 1, true, (0xa000_0000_0000_0000, 0x4001)),
        (0xd8, 0xe8, 7, false, (0xa000_0000_0000_0000, 0x4001)),
        (0xdc, 0xe0, 1, false, (0xa000_0000_0000_0000, 0xc001)),
        (0xde, 0xe0, 1, true, (0xa000_0000_0000_0000, 0xc001)),
    ] {
        for alias in [false, true] {
            let code = [prefix, base + if alias { 0 } else { 2 }];
            let mut image = stack_image(&code, 7, 0);
            write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x4000));
            write_value(&mut image.cpu, 7, (LEADING, 0xc000));
            let opcode = (u16::from(prefix & 7) << 8) | u16::from(code[1]);
            let mut result_cpu = complete_x87(image.cpu, 2, opcode);
            result_cpu.x87.status.c1 = 0;
            let bits = if alias {
                if base == 0xc0 {
                    (LEADING, 0xc001)
                } else {
                    (0, 0)
                }
            } else {
                result
            };
            write_value(&mut result_cpu, if alias { 7 } else { destination }, bits);
            if pop {
                result_cpu.x87.tag_word |= 0xc000;
                result_cpu.x87.status.top = 0;
            }
            checks.check(
                "register operand order and aliases",
                &code,
                &image,
                &[dispatch(result_cpu)],
            );
        }
    }
}

fn signed_zeros(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (instruction, negate_left, negate_right) in [
        ([0xde, 0xc1], false, false),
        ([0xde, 0xe9], false, true),
        ([0xde, 0xe1], true, false),
    ] {
        for left_negative in [false, true] {
            for right_negative in [false, true] {
                for rc in 0..4 {
                    let a = left_negative ^ negate_left;
                    let b = right_negative ^ negate_right;
                    let negative = if a == b { a } else { rc == 1 };
                    check_arithmetic(
                        &mut checks,
                        instruction,
                        "signed zero operands",
                        Case {
                            left: (0, if left_negative { 0x8000 } else { 0 }),
                            right: (0, if right_negative { 0x8000 } else { 0 }),
                            control: 0x037f | (rc << 10),
                            result: Some((0, if negative { 0x8000 } else { 0 })),
                            flags: 0,
                        },
                    );
                }
            }
        }
    }
    // Adding zero still rounds the nonzero operand to PC, including C1 and #P.
    for (pc, unit) in [(0, 1_u64 << 40), (2, 1 << 11), (3, 1)] {
        for rc in 0..4 {
            for swap in [false, true] {
                let value = (LEADING + 1, 0x3fff);
                let bits = if pc == 3 {
                    value
                } else {
                    (LEADING + if rc == 2 { unit } else { 0 }, 0x3fff)
                };
                check_arithmetic(
                    &mut checks,
                    [0xde, 0xc1],
                    "zero partner retains precision rounding",
                    Case {
                        left: if swap { (0, 0x8000) } else { value },
                        right: if swap { value } else { (0, 0x8000) },
                        control: 0x007f | (pc << 8) | (rc << 10),
                        result: Some(bits),
                        flags: if pc == 3 {
                            0
                        } else {
                            PE | if rc == 2 { C1 } else { 0 }
                        },
                    },
                );
            }
        }
    }
}

test_frontends!(forms, register_forms);
test_frontends!(zeros, signed_zeros);
