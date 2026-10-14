//! F2XM1 retains binary80 precision, range, exception priority and restart state.

#[path = "x87_exponential/values.rs"]
mod values;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{expected, Exit, Step},
    step::{Engine, TestModule},
    x87::{complete_x87, dispatch, real80, set_control, stack_image, write_value, INDEFINITE},
};
use wasm86_x86::{Compiler, CpuState, SegmentProfile};

const CODE: [u8; 2] = [0xd9, 0xf0];
const LEAD: u64 = 1 << 63;

#[test]
fn encoding() {
    check_length(&CODE);
}

fn values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for &values::Reference {
        input,
        lower,
        upper,
        nearest_up,
        tiny,
    } in values::VALUES
    {
        for rc in 0..4 {
            for pc in [0, 2, 3] {
                let mut image = stack_image(&CODE, 7, 0xffff);
                set_control(&mut image.cpu.x87.control, 0x007f | (pc << 8) | (rc << 10));
                image.cpu.x87.status.precision = 0;
                write_value(&mut image.cpu, 7, input);
                let increment = lower != upper
                    && match rc {
                        0 => nearest_up,
                        1 => input.1 & 0x8000 != 0,
                        2 => input.1 & 0x8000 == 0,
                        _ => false,
                    };
                let mut result = complete_x87(image.cpu, 2, 0x01f0);
                result.x87.status.c1 = u8::from(increment);
                result.x87.status.precision = u8::from(input.0 != 0);
                result.x87.status.denormal = u8::from(input.0 != 0 && input.1 & 0x7fff == 0);
                result.x87.status.underflow = u8::from(tiny);
                write_value(&mut result, 7, if increment { upper } else { lower });
                checks.check(
                    &format!("input={input:x?} PC={pc} RC={rc}"),
                    &CODE,
                    &image,
                    &[dispatch(result)],
                );
            }
        }
    }
}

fn infinities(engine: Engine, frontend: Frontend) {
    let code = [0xd9, 0xf0, 0x9b];
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (input, output) in [
        ((LEAD, 0xffff), (LEAD, 0xbfff)),
        ((LEAD, 0x7fff), (LEAD, 0x7fff)),
    ] {
        for rc in 0..4 {
            let mut image = stack_image(&code, 3, 0xffff);
            set_control(&mut image.cpu.x87.control, 0x0340 | rc << 10);
            image.cpu.x87.status.precision = 0;
            write_value(&mut image.cpu, 3, input);
            let mut result = complete_x87(image.cpu, 2, 0x01f0);
            result.x87.status.c1 = 0;
            write_value(&mut result, 3, output);
            let mut waited = result;
            waited.eip += 1;
            waited.instruction_count += 1;
            checks.check(
                &format!("infinity={input:x?} RC={rc} has no unmasked exception"),
                &code,
                &image,
                &[dispatch(result), dispatch(waited)],
            );
        }
    }
}

fn exceptions(engine: Engine, frontend: Frontend) {
    let code = [0xd9, 0xf0, 0x9b];
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (input, output, flag) in [
        (None, INDEFINITE, 1_u16),
        (Some((42, 0x3fff)), INDEFINITE, 1),
        (
            Some((LEAD + 42, 0x7fff)),
            (0xc000_0000_0000_002a, 0x7fff),
            1,
        ),
        (
            Some((0xc000_0000_0000_0042, 0xffff)),
            (0xc000_0000_0000_0042, 0xffff),
            0,
        ),
        (Some((1, 0)), (1, 0), 2),
        (Some((LEAD, 1)), (0xb172_17f7_d1cf_79ac, 0x6000), 16),
        (Some((LEAD, 0x3ffe)), (0xd413_cccf_e779_9211, 0x3ffd), 32),
        (Some((LEAD, 0x3fff)), (LEAD, 0x3fff), 32),
    ] {
        let mut image = stack_image(&code, 3, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x037f & !flag);
        image.cpu.x87.status.precision = 0;
        if let Some(input) = input {
            write_value(&mut image.cpu, 3, input);
        }
        let suppressed = flag == 1 || flag == 2;
        let mut result = complete_x87(image.cpu, 2, 0x01f0);
        result.x87.status.invalid = u8::from(flag == 1);
        result.x87.status.stack_fault = u8::from(input.is_none());
        result.x87.status.denormal = u8::from(flag == 2);
        result.x87.status.underflow = u8::from(flag == 16);
        result.x87.status.precision = u8::from(flag == 16 || flag == 32);
        result.x87.status.c1 = u8::from(flag == 16);
        result.x87.status.error_summary = u8::from(flag != 0);
        result.x87.status.busy = u8::from(flag != 0);
        if !suppressed {
            write_value(&mut result, 3, output);
        }
        let waited = if flag != 0 {
            Step {
                cpu: result,
                ram: &[],
                exit: Exit::FloatingPoint,
            }
        } else {
            let mut completed = result;
            completed.eip += 1;
            completed.instruction_count += 1;
            dispatch(completed)
        };
        checks.check(
            &format!("input={input:x?} unmasked={flag:x}"),
            &code,
            &image,
            &[dispatch(result), waited],
        );
    }

    let mut image = stack_image(&CODE, 0, 0xfffc);
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "pending exception preserves metadata",
        &CODE,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

fn specialization(engine: Engine) {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let compiled = compiler.compile_block(0x1000, &CODE, 1).unwrap();
    let module = TestModule::new(&compiled);
    for input in [
        None,
        Some((1, 0)),
        Some((42, 0x3fff)),
        Some((LEAD, 1)),
        Some((LEAD, 0x4000)),
        Some((LEAD, 0xffff)),
        Some((LEAD, 0x7fff)),
    ] {
        let mut image = stack_image(&CODE, 2, 0xffff);
        if let Some(input) = input {
            write_value(&mut image.cpu, 2, input);
        }
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Interpret
                }]
            ),
            "rejected input={input:x?} restarts before effects"
        );
    }
    let mut observed = CpuState::default();
    let mut modules = Vec::new();
    for pc in [0, 2, 3] {
        observed.x87.control.precision_control = pc;
        modules.push(
            Compiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&observed)
                .compile_block(0x1000, &CODE, 1)
                .unwrap()
                .bytes,
        );
    }
    assert!(
        modules.windows(2).all(|pair| pair[0] == pair[1]),
        "PC cannot change transcendental precision"
    );

    // A tiny result restarts after the earlier load, without repeating its push.
    let code = [0xdb, 0x2d, 0, 0x40, 0, 0, 0xd9, 0xf0];
    let compiled = compiler.compile_block(0x1000, &code, 2).unwrap();
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.status.precision = 0;
    image.map(4, 0x8000, false);
    image.data(0x8000, &real80((LEAD, 1)));
    let mut prefix = complete_x87(image.cpu, 6, 0x032d);
    prefix.x87.status.top = 7;
    prefix.x87.status.c1 = 0;
    prefix.x87.data_offset = 0x4000;
    prefix.x87.data_selector = 0x23;
    write_value(&mut prefix, 7, (LEAD, 1));
    assert_eq!(
        engine.observe(&TestModule::new(&compiled), &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: prefix,
                ram: &[],
                exit: Exit::Interpret
            }]
        ),
    );
    let mut result = complete_x87(prefix, 2, 0x01f0);
    result.x87.status.underflow = 1;
    result.x87.status.precision = 1;
    result.x87.status.c1 = 1;
    write_value(&mut result, 7, (0x58b9_0bfb_e8e7_bcd6, 0));
    assert_eq!(
        engine.observe(
            &TestModule::new(&compiled).with_interpreter(TestModule::interpreter()),
            &image.input(),
            1,
        ),
        expected(&image, &[dispatch(result)]),
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

test_frontends!(rounding_and_extended_range, values);
test_frontends!(infinite_inputs, infinities);
test_frontends!(exception_priority, exceptions);
