//! Small increments keep their exponent through the logarithm and multiplication.

use super::values::Reference;
use super::*;

#[path = "epsilon_values.rs"]
mod fixtures;

const CODE: [u8; 2] = [0xd9, 0xf9];

#[test]
fn encoding() {
    check_length(&CODE);
}

fn values(frontend: Frontend) {
    check_values(frontend, CODE, fixtures::VALUES);
}

fn exceptions(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let one = (LEAD, 0x3fff);
    let quarter = (LEAD, 0x3ffd);
    let infinity = (LEAD, 0x7fff);
    let qnan = (0xc000_0000_0000_0042, 0xffff);
    for (y, x, result, flags) in [
        (one, (0, 0x8000), (0, 0x8000), 0),
        ((LEAD, 0xbfff), (0, 0x8000), (0, 0), 0),
        ((0, 0x8000), quarter, (0, 0x8000), 0),
        ((0, 0), (LEAD, 0xbffd), (0, 0x8000), 0),
        (infinity, (0, 0), INDEFINITE, 1),
        ((LEAD, 0xffff), (0, 0x8000), INDEFINITE, 1),
        (infinity, quarter, infinity, 0),
        (infinity, (LEAD, 0xbffd), (LEAD, 0xffff), 0),
        ((LEAD, 0xffff), (LEAD, 0xbffd), infinity, 0),
        (infinity, qnan, qnan, 0),
        (one, (LEAD + 42, 0x7fff), (0xc000_0000_0000_002a, 0x7fff), 1),
        (one, (42, 0x3ffd), INDEFINITE, 1),
        // First value beyond the specified domain retains y and completes the pop.
        (
            (0xc000_0000_0000_0000, 0x4000),
            (0x95f6_1998_0c43_36f8, 0x3ffd),
            (0xc000_0000_0000_0000, 0x4000),
            0,
        ),
        ((1, 0), (0, 0), (0, 0), 2),
    ] {
        for unmasked in [false, true] {
            let suppress = unmasked && flags != 0;
            check_arithmetic(
                &mut checks,
                CODE,
                "increment operand table",
                ArithmeticCase {
                    left: y,
                    right: x,
                    control: if unmasked { 0x0340 } else { 0x037f },
                    result: if suppress { None } else { Some(result) },
                    flags: flags | if suppress { 0x8080 } else { 0 },
                },
            );
        }
    }
    check_arithmetic(
        &mut checks,
        CODE,
        "unmasked tiny product uses the extended exponent",
        ArithmeticCase {
            left: one,
            right: (1, 0),
            control: 0x036f,
            result: Some((0xb8aa_3b29_5c17_f0bc, 0x5fc2)),
            flags: 0x82b2,
        },
    );
}

#[test]
fn jit_specialization() {
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &CODE, 1)
        .unwrap();
    let module = TestModule::new(&compiled);
    for x in [(LEAD, 0x3ffe), (0x95f6_1998_0c43_36f8, 0x3ffd), (1, 0)] {
        let mut image = stack_image(&CODE, 7, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 7, x);
        write_value(&mut image.cpu, 0, (LEAD, 0x3fff));
        image.check_unchanged_exit(
            &module,
            "increment guard restarts before effects",
            Exit::Interpret,
        );
    }
    check_pc_independent(CODE);
}

test_frontends!(values_and_extended_range, values);
test_frontends!(exceptions_and_pop, exceptions);
