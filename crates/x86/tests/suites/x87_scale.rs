//! FSCALE preserves the significand, truncates its scale, and handles extended range.

#[path = "x87_scale/jit.rs"]
mod jit;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::dispatch,
    x87::{complete_x87, set_control, stack_image, write_value, INDEFINITE},
};
use relocade_x86::SegmentProfile;

const LEAD: u64 = 1 << 63;
const CODE: [u8; 2] = [0xd9, 0xfd];

#[test]
fn encoding() {
    check_length(&CODE);
}

fn finite_values(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (scale, delta) in [
        ((0, 0), 0),
        ((LEAD, 0x3ffe), 0),
        ((LEAD, 0xbffe), 0),
        ((0xe000_0000_0000_0000, 0x3fff), 1),
        ((0xe000_0000_0000_0000, 0xbfff), -1),
        ((0xffff_ffff_ffff_ffff, 0x4000), 3),
        ((0xffff_ffff_ffff_ffff, 0xc000), -3),
        ((LEAD, 0x4009), 1024),
    ] {
        for sign in [0, 0x8000] {
            for pc in [0, 2, 3] {
                for rc in 0..4 {
                    let mut image = stack_image(&CODE, 7, 0xffff);
                    set_control(&mut image.cpu.x87.control, 0x007f | (pc << 8) | (rc << 10));
                    image.cpu.x87.status.precision = 0;
                    write_value(&mut image.cpu, 7, (LEAD + 1, 0x4000 | sign));
                    write_value(&mut image.cpu, 0, scale);
                    let mut result = complete_x87(image.cpu, 2, 0x01fd);
                    result.x87.status.c1 = 0;
                    write_value(
                        &mut result,
                        7,
                        (LEAD + 1, (0x4000_i32 + delta) as u16 | sign),
                    );
                    checks.check(
                        &format!("scale={scale:x?} PC={pc} RC={rc} sign={sign:x}"),
                        &CODE,
                        &image,
                        &[dispatch(result)],
                    );
                }
            }
        }
    }
}

fn ranges(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // Input, scale, truncated masked result, masked increment, adjusted result,
    // range flag, masked inexactness, and whether adjustment itself saturates.
    for (input, scale, lower, upper, adjusted, flag, inexact, massive) in [
        (
            (LEAD, 0x7ffe),
            (LEAD, 0x3fff),
            (u64::MAX, 0x7ffe),
            (LEAD, 0x7fff),
            (LEAD, 0x1fff),
            8,
            true,
            false,
        ),
        (
            (LEAD, 1),
            (LEAD, 0xbfff),
            (LEAD >> 1, 0),
            (LEAD >> 1, 0),
            (LEAD, 0x6000),
            16,
            false,
            false,
        ),
        (
            (LEAD, 1),
            (LEAD, 0xc005),
            (0, 0),
            (1, 0),
            (LEAD, 0x5fc1),
            16,
            true,
            false,
        ),
        (
            (LEAD, 0x3fff),
            (LEAD, 0x400f),
            (u64::MAX, 0x7ffe),
            (LEAD, 0x7fff),
            (LEAD, 0x7fff),
            8,
            true,
            true,
        ),
        (
            (LEAD, 0x3fff),
            (LEAD, 0xc00f),
            (0, 0),
            (1, 0),
            (0, 0),
            16,
            true,
            true,
        ),
        (
            (LEAD, 0x3fff),
            (0x9fff_0000_0000_0000, 0x400e),
            (u64::MAX, 0x7ffe),
            (LEAD, 0x7fff),
            (LEAD, 0x7ffe),
            8,
            true,
            false,
        ),
        (
            (LEAD, 0x3fff),
            (0x9ffe_0000_0000_0000, 0xc00e),
            (0, 0),
            (1, 0),
            (LEAD, 1),
            16,
            true,
            false,
        ),
        (
            (LEAD, 0x3fff),
            (0xa000_0000_0000_0000, 0x400e),
            (u64::MAX, 0x7ffe),
            (LEAD, 0x7fff),
            (LEAD, 0x7fff),
            8,
            true,
            true,
        ),
        (
            (LEAD, 0x3fff),
            (0x9fff_0000_0000_0000, 0xc00e),
            (0, 0),
            (1, 0),
            (0, 0),
            16,
            true,
            true,
        ),
        (
            (LEAD, 1),
            (u64::MAX, 0x7ffe),
            (u64::MAX, 0x7ffe),
            (LEAD, 0x7fff),
            (LEAD, 0x7fff),
            8,
            true,
            true,
        ),
        (
            (LEAD, 0x7ffe),
            (u64::MAX, 0xfffe),
            (0, 0),
            (1, 0),
            (0, 0),
            16,
            true,
            true,
        ),
    ] {
        for sign in [0, 0x8000] {
            for rc in 0..4 {
                for masked in [false, true] {
                    let mut image = stack_image(&CODE, 0, 0xffff);
                    set_control(
                        &mut image.cpu.x87.control,
                        (0x007f | (rc << 10)) & if masked { 0xffff } else { !flag },
                    );
                    image.cpu.x87.status.precision = 0;
                    write_value(&mut image.cpu, 0, (input.0, input.1 | sign));
                    write_value(&mut image.cpu, 1, scale);
                    let up = if masked {
                        inexact
                            && ((flag == 8 && rc == 0)
                                || (rc == 1 && sign != 0)
                                || (rc == 2 && sign == 0))
                    } else {
                        massive && flag == 8
                    };
                    let output = if masked {
                        if up {
                            upper
                        } else {
                            lower
                        }
                    } else {
                        adjusted
                    };
                    let mut result = complete_x87(image.cpu, 2, 0x01fd);
                    result.x87.status.overflow = u8::from(flag == 8);
                    result.x87.status.underflow = u8::from(flag == 16 && (!masked || inexact));
                    result.x87.status.precision = u8::from(if masked { inexact } else { massive });
                    result.x87.status.c1 = u8::from(up);
                    result.x87.status.error_summary = u8::from(!masked);
                    result.x87.status.busy = u8::from(!masked);
                    write_value(&mut result, 0, (output.0, output.1 | sign));
                    checks.check(
                        &format!(
                            "{input:x?} scale={scale:x?} RC={rc} sign={sign:x} masked={masked}"
                        ),
                        &CODE,
                        &image,
                        &[dispatch(result)],
                    );
                }
            }
        }
    }
}

fn specials(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (left, right, output, flags) in [
        ((LEAD, 0x3fff), (LEAD, 0x7fff), (LEAD, 0x7fff), 0),
        ((LEAD, 0xbfff), (LEAD, 0xffff), (0, 0x8000), 0),
        ((LEAD, 0x7fff), (LEAD, 0xffff), INDEFINITE, 1),
        ((0, 0x8000), (LEAD, 0x7fff), INDEFINITE, 1),
        ((LEAD, 0xffff), (LEAD, 0x7fff), (LEAD, 0xffff), 0),
        ((0, 0x8000), (LEAD, 0xffff), (0, 0x8000), 0),
        ((0, 0), (u64::MAX, 0x7ffe), (0, 0), 0),
        ((LEAD, 0xffff), (u64::MAX, 0xfffe), (LEAD, 0xffff), 0),
        ((LEAD, 0x3fff), (1, 0), (LEAD, 0x3fff), 2),
        ((LEAD, 0), (LEAD, 0x3fff), (LEAD, 2), 2),
        ((1, 0), (LEAD, 0x4005), (LEAD, 2), 2),
        (
            (LEAD + 1, 0x7fff),
            (1, 0),
            (0xc000_0000_0000_0001, 0x7fff),
            1,
        ),
        (
            (1, 0),
            (0xc000_0000_0000_0042, 0xffff),
            (0xc000_0000_0000_0042, 0xffff),
            0,
        ),
        ((1, 0x3fff), (0, 0), INDEFINITE, 1),
    ] {
        for masked in [false, true] {
            let mut image = stack_image(&CODE, 4, 0xffff);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037c },
            );
            image.cpu.x87.status.precision = 0;
            write_value(&mut image.cpu, 4, left);
            write_value(&mut image.cpu, 5, right);
            let suppressed = !masked && flags != 0;
            let mut result = complete_x87(image.cpu, 2, 0x01fd);
            result.x87.status.c1 = 0;
            result.x87.status.invalid = u8::from(flags == 1);
            result.x87.status.denormal = u8::from(flags == 2);
            result.x87.status.error_summary = u8::from(suppressed);
            result.x87.status.busy = u8::from(suppressed);
            if !suppressed {
                write_value(&mut result, 4, output);
            }
            checks.check(
                &format!("{left:x?}, {right:x?}, masked={masked}"),
                &CODE,
                &image,
                &[dispatch(result)],
            );
        }
    }
    for empty in [0, 1] {
        for masked in [false, true] {
            let mut image = stack_image(&CODE, 0, 0xffff);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037e },
            );
            write_value(&mut image.cpu, 1 - empty, (LEAD, 0x3fff));
            let mut result = complete_x87(image.cpu, 2, 0x01fd);
            result.x87.status.c1 = 0;
            result.x87.status.invalid = 1;
            result.x87.status.stack_fault = 1;
            result.x87.status.error_summary = u8::from(!masked);
            result.x87.status.busy = u8::from(!masked);
            if masked {
                write_value(&mut result, 0, INDEFINITE);
            }
            checks.check(
                "empty input keeps ST1 and TOP",
                &CODE,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

test_frontends!(finite, finite_values);
test_frontends!(range_responses, ranges);
test_frontends!(special_operands, specials);
