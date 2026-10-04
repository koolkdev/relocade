//! Register division keeps operand order, exact rounding and exception responses.

#[path = "x87_divide/exceptions.rs"]
mod exceptions;
#[path = "x87_divide/native.rs"]
mod native;
#[path = "x87_divide/rounding.rs"]
mod rounding;
#[path = "x87_divide/specialization.rs"]
mod specialization;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Step},
    step::Engine,
    x87::{
        arithmetic::{check_arithmetic, ArithmeticCase as Case},
        complete_x87, dispatch, set_control, stack_image, status, write_register_bits, write_value,
        INDEFINITE,
    },
};
use wasm86_x86::SegmentProfile;

const LEADING: u64 = 1 << 63;
const PE: u16 = 0x20;
const C1: u16 = 0x200;
const PENDING: u16 = 0x8080;

fn register_forms(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // ST0=-6 and ST2=3 distinguish dividend, divisor and destination. TOP=7
    // exercises the destination's old physical slot across a wrapping pop.
    for (prefix, base, destination, pop, exponent) in [
        (0xd8, 0xf0, 7, false, 0xc000),
        (0xdc, 0xf8, 1, false, 0xbffe),
        (0xde, 0xf8, 1, true, 0xbffe),
        (0xd8, 0xf8, 7, false, 0xbffe),
        (0xdc, 0xf0, 1, false, 0xc000),
        (0xde, 0xf0, 1, true, 0xc000),
    ] {
        for alias in [false, true] {
            let code = [prefix, base + if alias { 0 } else { 2 }];
            let mut image = stack_image(&code, 7, 0);
            write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x4000));
            write_value(&mut image.cpu, 7, (0xc000_0000_0000_0000, 0xc001));
            let opcode = (u16::from(prefix & 7) << 8) | u16::from(code[1]);
            let mut result = complete_x87(image.cpu, 2, opcode);
            result.x87.status.c1 = 0;
            write_value(
                &mut result,
                if alias { 7 } else { destination },
                (LEADING, if alias { 0x3fff } else { exponent }),
            );
            if pop {
                result.x87.tag_word |= 0xc000;
                result.x87.status.top = 0;
            }
            checks.check(
                "division order and aliases",
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

test_frontends!(forms, register_forms);
