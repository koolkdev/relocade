//! Both trigonometric components share operand exceptions, but round independently.

use super::x87_trigonometric::values;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{expected, Exit, Step},
    step::{Engine, TestModule},
    x87::{complete_x87, dispatch, set_control, stack_image, write_value, INDEFINITE},
};
use wasm86_x86::{Compiler, SegmentProfile};

const LEAD: u64 = 1 << 63;
const CODE: [u8; 2] = [0xd9, 0xfb];

#[test]
fn encoding() {
    check_length(&CODE);
}

fn values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
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

fn exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (input, output, invalid) in [
        ((LEAD, 0x7fff), INDEFINITE, true),
        ((42, 0x3fff), INDEFINITE, true),
        ((LEAD + 42, 0xffff), (0xc000_0000_0000_002a, 0xffff), true),
        (
            (0xc000_0000_0000_0042, 0x7fff),
            (0xc000_0000_0000_0042, 0x7fff),
            false,
        ),
    ] {
        for masked in [false, true] {
            let mut image = stack_image(&CODE, 3, 0xffff);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037e },
            );
            image.cpu.x87.status.precision = 0;
            write_value(&mut image.cpu, 3, input);
            let mut result = complete_x87(image.cpu, 2, 0x01fb);
            result.x87.status.invalid = u8::from(invalid);
            result.x87.status.c1 = 0;
            result.x87.status.c2 = 0;
            result.x87.status.error_summary = u8::from(invalid && !masked);
            result.x87.status.busy = result.x87.status.error_summary;
            if masked || !invalid {
                write_value(&mut result, 3, output);
                write_value(&mut result, 2, output);
                result.x87.status.top = 2;
            }
            checks.check(
                &format!("input={input:x?} masked={masked}"),
                &CODE,
                &image,
                &[dispatch(result)],
            );
        }
    }
    for (input, mask, sine, cosine, flags) in [
        ((1, 0), 2, (1, 0), (LEAD, 0x3fff), 2_u16),
        ((1, 0), 16, (LEAD, 0x5fc2), (LEAD, 0x3fff), 0x232),
        (
            (LEAD, 0x3fff),
            32,
            (0xd76a_a478_4867_7021, 0x3ffe),
            (0x8a51_407d_a834_5c92, 0x3ffe),
            0x220,
        ),
    ] {
        let code = [0xd9, 0xfb, 0x9b];
        let mut image = stack_image(&code, 3, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x037f & !mask);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 3, input);
        let mut result = complete_x87(image.cpu, 2, 0x01fb);
        result.x87.status.denormal = u8::from(flags & 2 != 0);
        result.x87.status.underflow = u8::from(flags & 16 != 0);
        result.x87.status.precision = u8::from(flags & 32 != 0);
        result.x87.status.c1 = u8::from(flags & 0x200 != 0);
        result.x87.status.c2 = 0;
        result.x87.status.error_summary = 1;
        result.x87.status.busy = 1;
        if mask != 2 {
            write_value(&mut result, 3, sine);
            write_value(&mut result, 2, cosine);
            result.x87.status.top = 2;
        }
        checks.check(
            &format!("unmasked={mask:x}"),
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
}

fn stack_and_range(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for empty in [false, true] {
        for full in [false, true] {
            for masked in [false, true] {
                let code = [0xd9, 0xfb, 0xd9, 0xd0];
                let mut image = stack_image(&code, 0, 0xffff);
                set_control(
                    &mut image.cpu.x87.control,
                    if masked { 0x037f } else { 0x037e },
                );
                image.cpu.x87.status.precision = 0;
                if !empty {
                    write_value(&mut image.cpu, 0, (LEAD, 0x403e));
                    image.cpu.x87.tag_word |= 2; // Preserve even an imported tag on the range exit.
                }
                if full {
                    write_value(&mut image.cpu, 7, (LEAD, 0x3fff));
                }
                let fault = empty || full;
                let mut result = complete_x87(image.cpu, 2, 0x01fb);
                result.x87.status.invalid = u8::from(fault);
                result.x87.status.stack_fault = u8::from(fault);
                result.x87.status.c1 = u8::from(full && !empty);
                result.x87.status.c2 = u8::from(!fault);
                result.x87.status.error_summary = u8::from(fault && !masked);
                result.x87.status.busy = result.x87.status.error_summary;
                if fault && masked {
                    write_value(&mut result, 0, INDEFINITE);
                    write_value(&mut result, 7, INDEFINITE);
                    result.x87.status.top = 7;
                }
                let followed = if fault && !masked {
                    Step {
                        cpu: result,
                        ram: &[],
                        exit: Exit::FloatingPoint,
                    }
                } else {
                    dispatch(complete_x87(result, 2, 0x01d0))
                };
                checks.check(
                    &format!("empty={empty} full={full} masked={masked}"),
                    &code,
                    &image,
                    &[dispatch(result), followed],
                );
            }
        }
    }
}

fn specialization(engine: Engine) {
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &CODE, 1)
        .unwrap();
    for (input, full) in [
        (None, false),
        (Some((1, 0)), false),
        (Some((LEAD, 0x403e)), false),
        (Some((LEAD, 0x3fff)), true),
    ] {
        let mut image = stack_image(&CODE, 0, 0xffff);
        if let Some(input) = input {
            write_value(&mut image.cpu, 0, input);
        }
        if full {
            write_value(&mut image.cpu, 7, (LEAD, 0x3fff));
        }
        image.check_unchanged_exit(
            engine,
            &TestModule::new(&compiled),
            "restart precedes both writes and TOP",
            Exit::Interpret,
        );
    }
    let code = [0xd9, 0xfb, 0xd8, 0xc1]; // FSINCOS; FADD ST0, ST1
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.status.precision = 0;
    write_value(&mut image.cpu, 0, (0, 0x8000));
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &code, 2)
        .unwrap();
    let mut result = complete_x87(image.cpu, 2, 0x01fb);
    result = complete_x87(result, 2, 0x00c1);
    result.x87.status.top = 7;
    result.x87.status.c1 = 0;
    result.x87.status.c2 = 0;
    write_value(&mut result, 7, (LEAD, 0x3fff));
    assert_eq!(
        engine.observe(&TestModule::new(&compiled), &image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

#[test]
fn jit_specialization() {
    specialization(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_jit_specialization() {
    specialization(Engine::V8);
}
test_frontends!(values_and_rounding, values);
test_frontends!(operand_exceptions, exceptions);
test_frontends!(stack_and_range_effects, stack_and_range);
