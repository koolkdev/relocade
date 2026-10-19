//! Obsolete coprocessor controls are integer no-ops on the P4 target.

use super::*;
use crate::support::x87::{complete_x87, stack_image, write_value};

#[test]
fn encoding() {
    for opcode in [0xe0, 0xe1, 0xe4] {
        check_length(&[0xdb, opcode]);
    }
}

fn no_effect(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for opcode in [0xe0, 0xe1, 0xe4] {
        let code = [0xdb, opcode, 0x9b];
        let mut image = initial_image(&code);
        pending(&mut image);
        let result = retire(image.cpu, 2);
        checks.check(
            "obsolete control preserves pending exception and every x87 field",
            &code,
            &image,
            &[
                dispatch(result),
                Step {
                    cpu: result,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                },
            ],
        );
    }
    let code = [0x66, 0xdb, 0xe4];
    let image = initial_image(&code);
    checks.check(
        "operand-size prefix does not change the compatibility no-op",
        &code,
        &image,
        &[dispatch(retire(image.cpu, 3))],
    );
}

fn wait_prefix(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for opcode in [0xe0, 0xe1, 0xe4] {
        let code = [0x9b, 0xdb, opcode];
        let mut image = initial_image(&code);
        let waited = retire(image.cpu, 1);
        checks.check(
            "explicit WAIT retires independently",
            &code,
            &image,
            &[dispatch(waited), dispatch(retire(waited, 2))],
        );
        pending(&mut image);
        checks.check(
            "explicit WAIT delivers pending exception before obsolete opcode",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::FloatingPoint,
            }],
        );
    }
}

fn tracked_state(frontend: Frontend) {
    let code = [0xd9, 0xe8, 0xdb, 0xe0, 0xdb, 0xe1, 0xdb, 0xe4, 0xd8, 0xc0]; // FLD1; controls; FADD ST0,ST0
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.status.precision = 0;
    let mut loaded = complete_x87(image.cpu, 2, 0x01e8);
    loaded.x87.status.top = 7;
    loaded.x87.status.c1 = 0;
    write_value(&mut loaded, 7, (1 << 63, 0x3fff));
    let first = retire(loaded, 2);
    let second = retire(first, 2);
    let third = retire(second, 2);
    let mut added = complete_x87(third, 2, 0x00c0);
    write_value(&mut added, 7, (1 << 63, 0x4000));
    ImageSequences::new(frontend, SegmentProfile::Flat32).check(
        "tracked arithmetic crosses obsolete controls unchanged",
        &code,
        &image,
        &[
            dispatch(loaded),
            dispatch(first),
            dispatch(second),
            dispatch(third),
            dispatch(added),
        ],
    );
}

test_frontends!(preserved_state, no_effect);
test_frontends!(explicit_wait, wait_prefix);
test_frontends!(tracked_values, tracked_state);
