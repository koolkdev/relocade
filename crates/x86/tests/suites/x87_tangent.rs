//! Tangent preserves low significand bits, pole reductions, and directed tiny tails.

#[path = "x87_tangent/values.rs"]
mod values;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::dispatch,
    x87::{complete_x87, set_control, stack_image, write_value},
};
use relocade_x86::SegmentProfile;

const CODE: [u8; 2] = [0xd9, 0xf2];

#[test]
fn encoding() {
    check_length(&CODE);
}

fn values(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for &(input, ref bounds) in values::VALUES {
        for negative in [false, true] {
            for rc in 0..4 {
                let input = (input.0, input.1 | if negative { 0x8000 } else { 0 });
                let mut image = stack_image(&CODE, 7, 0xffff);
                set_control(&mut image.cpu.x87.control, 0x007f | rc << 10);
                image.cpu.x87.status.precision = 0;
                write_value(&mut image.cpu, 7, input);
                let (tangent, incremented) = bounds.rounded(rc, negative);
                let mut result = complete_x87(image.cpu, 2, 0x01f2);
                result.x87.status.c1 = u8::from(incremented);
                result.x87.status.c2 = 0;
                result.x87.status.precision = u8::from(input.0 != 0);
                result.x87.status.denormal = u8::from(input.0 != 0 && input.1 & 0x7fff == 0);
                // These subnormal inputs stay tiny when rounded with an
                // unbounded exponent, even if the final directed rounding
                // carries the largest subnormal into the minimum normal.
                result.x87.status.underflow = result.x87.status.denormal;
                write_value(&mut result, 7, tangent);
                write_value(&mut result, 6, (1 << 63, 0x3fff));
                result.x87.status.top = 6;
                checks.check(
                    &format!("input={input:x?} RC={rc}"),
                    &CODE,
                    &image,
                    &[dispatch(result)],
                );
            }
        }
    }
}

test_frontends!(values_and_rounding, values);
