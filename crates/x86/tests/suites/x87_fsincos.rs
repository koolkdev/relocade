//! Both trigonometric components share operand exceptions, but round independently.

use super::x87_trigonometric::values;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    x87::{complete_x87, dispatch, set_control, stack_image, write_value},
};
use relocade_x86::SegmentProfile;

const CODE: [u8; 2] = [0xd9, 0xfb];

#[test]
fn encoding() {
    check_length(&CODE);
}

fn values(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for reference in values::VALUES {
        for negative in [false, true] {
            for rc in 0..4 {
                let input = (
                    reference.input.0,
                    reference.input.1 | if negative { 0x8000 } else { 0 },
                );
                let mut image = stack_image(&CODE, 0, 0xffff);
                set_control(&mut image.cpu.x87.control, 0x007f | rc << 10);
                image.cpu.x87.status.precision = 0;
                write_value(&mut image.cpu, 0, input);
                let (sine, _) = reference.sine.rounded(rc, negative);
                let (cosine, incremented) = reference.cosine.rounded(rc, false);
                let mut result = complete_x87(image.cpu, 2, 0x01fb);
                result.x87.status.c1 = u8::from(incremented);
                result.x87.status.c2 = 0;
                result.x87.status.precision = u8::from(input.0 != 0);
                result.x87.status.denormal = u8::from(input.0 != 0 && input.1 & 0x7fff == 0);
                result.x87.status.underflow = u8::from(input.0 != 0 && sine.1 & 0x7fff == 0);
                write_value(&mut result, 0, sine);
                write_value(&mut result, 7, cosine);
                result.x87.status.top = 7;
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
