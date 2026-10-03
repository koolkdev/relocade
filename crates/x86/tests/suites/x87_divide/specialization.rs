//! Direct block observations require admitted quotients to stay in generated code.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::{compile_block_from_bytes, BlockCompiler, CpuState};

fn controls(engine: Engine) {
    let code = [0xde, 0xf9];
    let dynamic = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    for (pc, precision) in [(0, 24), (1, 64), (2, 53), (3, 64)] {
        for rc in 0..4 {
            let mut observed = CpuState::default();
            observed.x87.control.precision_control = pc;
            observed.x87.control.rounding_control = rc;
            let specialized = TestModule::new(
                &BlockCompiler::new(SegmentProfile::Flat32)
                    .specialize_on_cpu(&observed)
                    .compile(0x1000, &code, 1)
                    .unwrap(),
            );
            for negative in [false, true] {
                for masked in [false, true] {
                    let numerator = LEADING;
                    let denominator = 0xc000_0000_0000_0000;
                    let (bits, flags) = rounding::exact_quotient(
                        numerator,
                        denominator,
                        precision,
                        u16::from(rc),
                        negative,
                    );
                    let mut image = stack_image(&code, 7, 0x3ffc);
                    image.cpu.x87.status.precision = 0;
                    set_control(
                        &mut image.cpu.x87.control,
                        0x005f
                            | (u16::from(pc) << 8)
                            | (u16::from(rc) << 10)
                            | if masked { PE } else { 0 },
                    );
                    write_value(
                        &mut image.cpu,
                        0,
                        (numerator, 0x3fff | if negative { 0x8000 } else { 0 }),
                    );
                    write_value(&mut image.cpu, 7, (denominator, 0x3fff));
                    let mut result = complete_x87(image.cpu, 2, 0x06f9);
                    write_value(&mut result, 0, bits);
                    result.x87.tag_word |= 0xc000;
                    result.x87.status = status(0x4500 | flags | if masked { 0 } else { PENDING });
                    for block in [&dynamic, &specialized] {
                        assert_eq!(engine.observe(block, &image.input(), 1), expected(&image, &[dispatch(result)]),
                            "division stays in the JIT: PC={pc} RC={rc} negative={negative} masked={masked}");
                    }
                }
            }
        }
    }
}

fn zeros_and_restart(engine: Engine) {
    let code = [0xd8, 0xf1];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    for numerator_sign in [0, 0x8000] {
        for denominator_sign in [0, 0x8000] {
            let mut image = stack_image(&code, 0, 0xfff0);
            write_value(&mut image.cpu, 0, (0, numerator_sign));
            write_value(&mut image.cpu, 1, (LEADING + 1, 0x3fff | denominator_sign));
            let mut result = complete_x87(image.cpu, 2, 0x00f1);
            result.x87.status.c1 = 0;
            write_value(&mut result, 0, (0, numerator_sign ^ denominator_sign));
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
    let code = [0xd8, 0xc9, 0xd8, 0xf2]; // An inexact multiply precedes the rejected divisor.
    let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for masked in [false, true] {
        let mut image = stack_image(&code, 0, 0xffc0);
        image.cpu.x87.status.precision = 0;
        set_control(
            &mut image.cpu.x87.control,
            if masked { 0x037f } else { 0x037b },
        );
        write_value(&mut image.cpu, 0, (LEADING + 1, 0x3fff));
        write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x3fff));
        write_value(&mut image.cpu, 2, (0, 0x8000));
        let mut first = complete_x87(image.cpu, 2, 0x00c9);
        first.x87.status.precision = 1;
        first.x87.status.c1 = 1;
        write_value(&mut first, 0, (0xc000_0000_0000_0002, 0x3fff));
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: first,
                    ram: &[],
                    exit: Exit::Interpret,
                }]
            )
        );
        let mut divided = complete_x87(first, 2, 0x00f2);
        divided.x87.status.zero_divide = 1;
        divided.x87.status.c1 = 0;
        if masked {
            write_value(&mut divided, 0, (LEADING, 0xffff));
        } else {
            divided.x87.status.error_summary = 1;
            divided.x87.status.busy = 1;
        }
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(divided)])
        );
    }
}

#[test]
fn admitted_quotients_use_dynamic_and_observed_modes() {
    controls(Engine::Wasmtime);
}

#[test]
fn zeros_and_rejected_divisors_preserve_restart_state() {
    zeros_and_restart(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_division_specialization_preserves_modes_and_restart() {
    controls(Engine::V8);
    zeros_and_restart(Engine::V8);
}
