//! FNOP waits and records its location without touching the stack or status.

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{dispatch, Exit, Step},
    x87::{complete_x87, set_control, stack_image},
};
use relocade_x86::SegmentProfile;

fn no_operation(frontend: Frontend) {
    let code = [0xd9, 0xd0];
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for tags in [0, 0xffff] {
        let mut image = stack_image(&code, 3, tags);
        // Retaining FNOP's undefined condition codes is emulator policy.
        image.cpu.x87.status.c0 = 0x81;
        image.cpu.x87.status.c1 = 0x82;
        image.cpu.x87.status.c2 = 0x83;
        image.cpu.x87.status.c3 = 0x84;
        let cpu = complete_x87(image.cpu, 2, 0x01d0);
        checks.check(
            "FNOP preserves full or empty stacks and all status fields",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
    let mut image = stack_image(&code, 0, 0xffff);
    set_control(&mut image.cpu.x87.control, 0x037e);
    image.cpu.x87.status.invalid = 1;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "FNOP delivers a pending exception before metadata and retirement",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

#[test]
fn encoding() {
    crate::support::encoding::check_length(&[0xd9, 0xd0]);
}

test_frontends!(effects, no_operation);
