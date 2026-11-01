//! FSQRT rounds once to the selected precision across the extended range.

#[path = "x87_sqrt/jit.rs"]
mod jit;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{dispatch, Exit, Step},
    x87::{complete_x87, set_control, stack_image, write_value, INDEFINITE},
};
use relocade_x86::SegmentProfile;

const LEAD: u64 = 1 << 63;
const CODE: [u8; 2] = [0xd9, 0xfa];

#[test]
fn encoding() {
    check_length(&CODE);
}

/// Compare the exact radicand against representable roots and their squared
/// midpoint. This does not use a host floating square root or guest rounding code.
fn expected_root(input: (u64, u16), pc: u16, rc: u16) -> ((u64, u16), bool, bool) {
    if input.0 == 0 {
        return (input, false, false);
    }
    let shift = input.0.leading_zeros();
    let exponent = i32::from(input.1.max(1)) - 16383 - shift as i32;
    let radicand = u128::from(input.0 << shift) << (63 + (exponent & 1));
    let discard = match pc {
        0 => 40,
        2 => 11,
        _ => 0,
    };
    let floor = radicand.isqrt() >> discard << discard;
    let inexact = floor * floor != radicand;
    let nearest_up = if discard == 0 {
        // No integer radicand has a half-integer root.
        radicand > floor * floor + floor
    } else {
        let midpoint = floor + (1 << (discard - 1));
        radicand > midpoint * midpoint
            || (radicand == midpoint * midpoint && (floor >> discard) & 1 != 0)
    };
    let increment = inexact
        && match rc {
            0 => nearest_up,
            2 => true,
            _ => false,
        };
    let rounded = floor + (u128::from(increment) << discard);
    let carry = rounded > u128::from(u64::MAX);
    (
        (
            if carry { LEAD } else { rounded as u64 },
            ((exponent >> 1) + 16383 + i32::from(carry)) as u16,
        ),
        inexact,
        increment,
    )
}

fn rounding(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let even_tie = LEAD + (1 << 40) + (1 << 15);
    let odd_tie = LEAD + (3 << 40) + (9 << 15);
    for input in [
        (0, 0),
        (0, 0x8000),
        (LEAD, 0x3fff),
        (LEAD, 0x4001),
        (0x9000_0000_0000_0000, 0x4002),
        (LEAD, 0x4000),
        (LEAD + 1, 0x3fff),
        (even_tie - 1, 0x3fff),
        (even_tie, 0x3fff),
        (even_tie + 1, 0x3fff),
        (odd_tie, 0x3fff),
        (0xdcce_60bd_0e0a_87ff, 0x4000),
        (u64::MAX, 0x4000),
        (LEAD, 1),
        (u64::MAX, 0x7ffe),
        (1, 0),
        (LEAD - 1, 0),
        (LEAD, 0),
    ] {
        for pc in 0..4 {
            for rc in 0..4 {
                let mut image = stack_image(&CODE, 7, 0xffff);
                set_control(&mut image.cpu.x87.control, 0x007f | (pc << 8) | (rc << 10));
                image.cpu.x87.status.precision = 0;
                write_value(&mut image.cpu, 7, input);
                let (output, inexact, increment) = expected_root(input, pc, rc);
                let mut result = complete_x87(image.cpu, 2, 0x01fa);
                result.x87.status.denormal = u8::from(input.1 == 0 && input.0 != 0);
                result.x87.status.precision = u8::from(inexact);
                result.x87.status.c1 = u8::from(increment);
                write_value(&mut result, 7, output);
                checks.check(
                    &format!("{input:x?} PC={pc} RC={rc}"),
                    &CODE,
                    &image,
                    &[dispatch(result)],
                );
            }
        }
    }
}

fn exceptions(frontend: Frontend) {
    let code = [0xd9, 0xfa, 0x9b];
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (input, output, flag, inexact, increment) in [
        (Some((LEAD, 0x7fff)), (LEAD, 0x7fff), 0, false, false),
        (Some((LEAD, 0xffff)), INDEFINITE, 1, false, false),
        (Some((LEAD, 0xbfff)), INDEFINITE, 1, false, false),
        (Some((1, 0x8000)), INDEFINITE, 1, false, false), // #I suppresses #D.
        (
            Some((0xc000_0000_0000_0042, 0xffff)),
            (0xc000_0000_0000_0042, 0xffff),
            0,
            false,
            false,
        ),
        (
            Some((LEAD + 42, 0xffff)),
            (0xc000_0000_0000_002a, 0xffff),
            1,
            false,
            false,
        ),
        (Some((42, 0x3fff)), INDEFINITE, 1, false, false),
        (Some((2, 0)), (LEAD, 0x1fe1), 2, false, false), // Exact root of 2^-16444.
        (Some((LEAD + 1, 0)), (LEAD, 0x2000), 2, true, false),
        (
            Some((LEAD, 0x4000)),
            (0xb504_f333_f9de_6484, 0x3fff),
            32,
            true,
            false,
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
            let mut result = complete_x87(image.cpu, 2, 0x01fa);
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
            let wait = if pending {
                Step {
                    cpu: result,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                }
            } else {
                let mut waited = result;
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

test_frontends!(precision_and_rounding, rounding);
test_frontends!(exception_responses, exceptions);
