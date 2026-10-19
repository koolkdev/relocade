//! Logarithmic products retain precision before multiplication and pop on completion.

#[path = "x87_logarithm/epsilon.rs"]
mod epsilon;
#[path = "x87_logarithm/values.rs"]
mod values;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{expected, Exit},
    step::TestModule,
    x87::{
        arithmetic::{check_arithmetic, ArithmeticCase},
        complete_x87, dispatch, stack_image, write_value, INDEFINITE,
    },
};
use wasm86_x86::{Compiler, CpuState, SegmentProfile};

const CODE: [u8; 2] = [0xd9, 0xf1];
const LEAD: u64 = 1 << 63;

#[test]
fn encoding() {
    check_length(&CODE);
}

fn values(frontend: Frontend) {
    check_values(frontend, CODE, values::VALUES);
}

fn check_values(frontend: Frontend, code: [u8; 2], references: &[values::Reference]) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for reference in references {
        let values::Reference {
            y,
            x,
            lower,
            upper,
            nearest_up,
            tiny,
        } = *reference;
        for rc in 0..4 {
            let negative = lower.1 & 0x8000 != 0;
            let increment = lower != upper
                && match rc {
                    0 => nearest_up,
                    1 => negative,
                    2 => !negative,
                    _ => false,
                };
            let denormal = (y.0 != 0 && y.1 & 0x7fff == 0) || (x.0 != 0 && x.1 & 0x7fff == 0);
            check_arithmetic(
                &mut checks,
                code,
                "extended logarithm before multiplication",
                ArithmeticCase {
                    left: y,
                    right: x,
                    control: 0x007f | (rc << 10),
                    result: Some(if increment { upper } else { lower }),
                    flags: (u16::from(increment) << 9)
                        | (u16::from(lower.0 != 0 || upper.0 != 0) << 5)
                        | (u16::from(tiny) << 4)
                        | (u16::from(denormal) << 1),
                },
            );
        }
    }
}

fn exceptions(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let one = (LEAD, 0x3fff);
    let infinity = (LEAD, 0x7fff);
    let qnan = (0xc000_0000_0000_0042, 0xffff);
    for (y, x, result, flags) in [
        (one, (0, 0), (LEAD, 0xffff), 4),
        ((LEAD, 0xbfff), (0, 0x8000), infinity, 4),
        (infinity, (0, 0), (LEAD, 0xffff), 0),
        ((1, 0), (0, 0), (LEAD, 0xffff), 4), // #Z takes priority over #D.
        ((LEAD, 0xffff), (0, 0x8000), infinity, 0),
        ((0, 0), (0, 0), INDEFINITE, 1),
        (one, (LEAD, 0xbfff), INDEFINITE, 1),
        (one, (LEAD, 0xffff), INDEFINITE, 1),
        ((0, 0), infinity, INDEFINITE, 1),
        (infinity, one, INDEFINITE, 1),
        ((LEAD, 0xbfff), one, (0, 0x8000), 0),
        ((0, 0), (LEAD, 0x3ffe), (0, 0x8000), 0),
        ((0, 0x8000), (LEAD, 0x4000), (0, 0x8000), 0),
        (infinity, (LEAD, 0x3ffe), (LEAD, 0xffff), 0),
        (one, infinity, infinity, 0),
        (qnan, (LEAD, 0xbfff), qnan, 0),
        ((0, 0), qnan, qnan, 0),
        ((1, 0), qnan, qnan, 0),
        (one, (LEAD + 42, 0x7fff), (0xc000_0000_0000_002a, 0x7fff), 1),
        (one, (42, 0x3fff), INDEFINITE, 1),
    ] {
        for unmasked in [false, true] {
            let suppress = unmasked && flags != 0;
            check_arithmetic(
                &mut checks,
                CODE,
                "operand table and deferred exceptions",
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
        "unmasked precision still writes and pops",
        ArithmeticCase {
            left: one,
            right: (LEAD, 0x4000),
            control: 0x035f,
            result: Some(one),
            flags: 0x80a0,
        },
    );
    check_arithmetic(
        &mut checks,
        CODE,
        "unmasked denormal suppresses write and pop",
        ArithmeticCase {
            left: one,
            right: (1, 0),
            control: 0x037d,
            result: None,
            flags: 0x8082,
        },
    );
    check_arithmetic(
        &mut checks,
        CODE,
        "exact tiny product adjusts exponent without a rounding increment",
        ArithmeticCase {
            left: (1, 0),
            right: (LEAD, 0x4000),
            control: 0x0b6f,
            result: Some((LEAD, 0x5fc2)),
            flags: 0x80b2,
        },
    );
    check_arithmetic(
        &mut checks,
        CODE,
        "unmasked overflow adjusts exponent and pops",
        ArithmeticCase {
            left: (LEAD, 0x7ffe),
            right: (LEAD, 0x4001),
            control: 0x0377,
            result: Some((LEAD, 0x1fff)),
            flags: 0x80a8,
        },
    );
}

#[test]
fn jit_specialization() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let compiled = compiler.compile_block(0x1000, &CODE, 1).unwrap();
    let module = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    let mut image = stack_image(&CODE, 7, 0xffff);
    image.cpu.x87.status.precision = 0;
    write_value(&mut image.cpu, 7, (0, 0));
    write_value(&mut image.cpu, 0, (LEAD, 0x3fff));
    image.check_unchanged_exit(
        &module,
        "zero log argument restarts before the pop",
        Exit::Interpret,
    );
    let mut result = complete_x87(image.cpu, 2, 0x01f1);
    result.x87.status.c1 = 0;
    result.x87.status.zero_divide = 1;
    result.x87.status.top = 0;
    result.x87.tag_word |= 0xc000;
    write_value(&mut result, 0, (LEAD, 0xffff));
    assert_eq!(
        linked.observe(&image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
    check_pc_independent(CODE);
}

fn check_pc_independent(code: [u8; 2]) {
    let mut modules = Vec::new();
    for pc in [0, 2, 3] {
        let mut cpu = CpuState::default();
        cpu.x87.control.precision_control = pc;
        modules.push(
            Compiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&cpu)
                .compile_block(0x1000, &code, 1)
                .unwrap()
                .bytes,
        );
    }
    assert!(modules.windows(2).all(|pair| pair[0] == pair[1]));
}

test_frontends!(values_and_cancellation, values);
test_frontends!(exceptions_and_pop, exceptions);
