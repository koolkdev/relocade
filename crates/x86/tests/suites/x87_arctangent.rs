//! FPATAN covers quadrants without performing an architectural division.

#[path = "x87_arctangent/values.rs"]
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

const CODE: [u8; 2] = [0xd9, 0xf3];
const LEAD: u64 = 1 << 63;

#[test]
fn encoding() {
    check_length(&CODE);
}

fn values(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for &values::Reference {
        y,
        x,
        lower,
        upper,
        nearest_up,
    } in values::VALUES
    {
        for rc in 0..4 {
            let negative = lower.1 & 0x8000 != 0;
            let increment = match rc {
                0 => nearest_up,
                1 => negative,
                2 => !negative,
                _ => false,
            };
            let output = if increment { upper } else { lower };
            let denormal = (y.1 & 0x7fff == 0) || (x.1 & 0x7fff == 0);
            check_arithmetic(
                &mut checks,
                CODE,
                "quadrant and extended ratio",
                ArithmeticCase {
                    left: y,
                    right: x,
                    control: 0x007f | (rc << 10),
                    result: Some(output),
                    flags: 0x20
                        | (u16::from(increment) << 9)
                        | (u16::from(denormal) << 1)
                        | (u16::from(output.1 & 0x7fff == 0) << 4),
                },
            );
        }
    }
}

fn axes(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let pi = (0xc90f_daa2_2168_c235, 0x4000);
    let half_pi = (pi.0, 0x3fff);
    let quarter_pi = (pi.0, 0x3ffe);
    let three_quarters = (0x96cb_e3f9_990e_91a8, 0x4000);
    for y in [(0, 0), (LEAD, 0x3fff), (LEAD, 0x7fff)] {
        for x in [(0, 0), (LEAD, 0x3fff), (LEAD, 0x7fff)] {
            if x.1 == 0x3fff && y.1 == 0x3fff {
                continue;
            }
            for sy in [0, 0x8000] {
                for sx in [0, 0x8000] {
                    let y = (y.0, y.1 | sy);
                    let x = (x.0, x.1 | sx);
                    let output = if x.1 & 0x7fff == 0x7fff && y.1 & 0x7fff == 0x7fff {
                        if sx != 0 {
                            three_quarters
                        } else {
                            quarter_pi
                        }
                    } else if y.0 == 0 || x.1 & 0x7fff == 0x7fff {
                        if sx != 0 {
                            pi
                        } else {
                            (0, 0)
                        }
                    } else {
                        half_pi
                    };
                    check_arithmetic(
                        &mut checks,
                        CODE,
                        "axes and infinite quadrants",
                        ArithmeticCase {
                            left: y,
                            right: x,
                            control: 0x037f,
                            result: Some((output.0, output.1 | sy)),
                            flags: if output.0 == 0 { 0 } else { 0x220 },
                        },
                    );
                }
            }
        }
    }
    for (y, x, result, flags) in [
        (
            (LEAD + 42, 0x7fff),
            (0, 0),
            (0xc000_0000_0000_002a, 0x7fff),
            1,
        ),
        ((LEAD, 0x3fff), (42, 0x3fff), INDEFINITE, 1),
        (
            (1, 0),
            (0xc000_0000_0000_0042, 0xffff),
            (0xc000_0000_0000_0042, 0xffff),
            0,
        ),
    ] {
        check_arithmetic(
            &mut checks,
            CODE,
            "operand exception priority",
            ArithmeticCase {
                left: y,
                right: x,
                control: 0x037f,
                result: Some(result),
                flags,
            },
        );
    }
    for sign in [0, 0x8000] {
        for rc in 0..4 {
            let increment = (rc == 1 && sign != 0) || (rc == 2 && sign == 0);
            check_arithmetic(
                &mut checks,
                CODE,
                "atan tail moves below a subnormal midpoint",
                ArithmeticCase {
                    left: (3, sign),
                    right: (LEAD, 0x4000),
                    control: 0x037f | (rc << 10),
                    result: Some((if increment { 2 } else { 1 }, sign)),
                    flags: 0x32 | (u16::from(increment) << 9),
                },
            );
        }
    }
    check_arithmetic(
        &mut checks,
        CODE,
        "unmasked underflow publishes adjusted angle and pops",
        ArithmeticCase {
            left: (1, 0),
            right: (LEAD, 0x3fff),
            control: 0x036f,
            result: Some((LEAD, 0x5fc2)),
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
    let mut image = stack_image(&CODE, 7, 0xffff);
    image.cpu.x87.status.precision = 0;
    write_value(&mut image.cpu, 7, (LEAD, 0x3fff));
    write_value(&mut image.cpu, 0, (1, 0));
    image.check_unchanged_exit(
        &module,
        "tiny angle restarts before the pop",
        Exit::Interpret,
    );
    let mut result = complete_x87(image.cpu, 2, 0x01f3);
    result.x87.status.denormal = 1;
    result.x87.status.underflow = 1;
    result.x87.status.precision = 1;
    result.x87.status.c1 = 1;
    result.x87.status.top = 0;
    result.x87.tag_word |= 0xc000;
    write_value(&mut result, 0, (1, 0));
    assert_eq!(
        TestModule::new(&compiled)
            .with_interpreter(TestModule::interpreter())
            .observe(&image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
    let mut bytes = Vec::new();
    for pc in [0, 2, 3] {
        let mut cpu = CpuState::default();
        cpu.x87.control.precision_control = pc;
        bytes.push(
            Compiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&cpu)
                .compile_block(0x1000, &CODE, 1)
                .unwrap()
                .bytes,
        );
    }
    assert!(bytes.windows(2).all(|pair| pair[0] == pair[1]));
}

test_frontends!(values_and_rounding, values);
test_frontends!(axes_and_exceptions, axes);
