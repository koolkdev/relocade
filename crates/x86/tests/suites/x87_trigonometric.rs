//! Trigonometric reduction, final rounding, range flags, and precise restarts.

#[path = "x87_trigonometric/values.rs"]
mod values;

use crate::support::{
    encoding::check_length,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{expected, Exit, Step},
    step::{Engine, TestModule},
    x87::{complete_x87, dispatch, set_control, stack_image, write_value, INDEFINITE},
};
use wasm86_x86::{Compiler, CpuState, SegmentProfile};

const LEAD: u64 = 1 << 63;

impl values::Bounds {
    fn rounded(&self, rc: u16, flip_sign: bool) -> ((u64, u16), bool) {
        let negative = (self.lower.1 & 0x8000 != 0) ^ flip_sign;
        let increment = self.lower != self.upper
            && match rc {
                0 => self.nearest_up,
                1 => negative,
                2 => !negative,
                _ => false,
            };
        let output = if increment { self.upper } else { self.lower };
        (
            (output.0, output.1 ^ if flip_sign { 0x8000 } else { 0 }),
            increment,
        )
    }
}

#[test]
fn encoding() {
    for opcode in [0xfe, 0xff] {
        check_length(&[0xd9, opcode]);
    }
}

fn values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for reference in values::VALUES {
        for negative in [false, true] {
            let input = (
                reference.input.0,
                reference.input.1 | if negative { 0x8000 } else { 0 },
            );
            for (opcode, bounds) in [(0xfe, &reference.sine), (0xff, &reference.cosine)] {
                for rc in 0..4 {
                    let code = [0xd9, opcode];
                    let mut image = stack_image(&code, 7, 0xffff);
                    set_control(&mut image.cpu.x87.control, 0x007f | (rc << 10));
                    image.cpu.x87.status.precision = 0;
                    write_value(&mut image.cpu, 7, input);
                    let (output, increment) = bounds.rounded(rc, opcode == 0xfe && negative);
                    let mut result = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
                    result.x87.status.c1 = u8::from(increment);
                    result.x87.status.c2 = 0;
                    result.x87.status.precision = u8::from(input.0 != 0);
                    result.x87.status.denormal = u8::from(input.0 != 0 && input.1 & 0x7fff == 0);
                    result.x87.status.underflow = u8::from(input.0 != 0 && output.1 & 0x7fff == 0);
                    write_value(&mut result, 7, output);
                    checks.check(
                        &format!("opcode={opcode:x} input={input:x?} RC={rc}"),
                        &code,
                        &image,
                        &[dispatch(result)],
                    );
                }
            }
        }
    }
}

fn exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for opcode in [0xfe, 0xff] {
        let code = [0xd9, opcode];
        for (input, output, invalid, range) in [
            (None, INDEFINITE, true, false),
            (Some((42, 0x3fff)), INDEFINITE, true, false),
            (Some((LEAD, 0x7fff)), INDEFINITE, true, false),
            (
                Some((LEAD + 42, 0xffff)),
                (0xc000_0000_0000_002a, 0xffff),
                true,
                false,
            ),
            (
                Some((0xc000_0000_0000_0042, 0x7fff)),
                (0xc000_0000_0000_0042, 0x7fff),
                false,
                false,
            ),
            (Some((LEAD, 0x403e)), (LEAD, 0x403e), false, true),
            (Some((u64::MAX, 0xfffe)), (u64::MAX, 0xfffe), false, true),
        ] {
            let mut image = stack_image(&code, 3, 0xffff);
            image.cpu.x87.status.precision = 0;
            if let Some(input) = input {
                write_value(&mut image.cpu, 3, input);
            }
            let mut result = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
            result.x87.status.invalid = u8::from(invalid);
            result.x87.status.stack_fault = u8::from(input.is_none());
            result.x87.status.c1 = 0;
            result.x87.status.c2 = u8::from(range);
            write_value(&mut result, 3, output);
            checks.check(
                &format!("opcode={opcode:x} exceptional input={input:x?}"),
                &code,
                &image,
                &[dispatch(result)],
            );
        }
        for (input, mask, output, flags) in [
            (None, 1, INDEFINITE, 1_u16),
            (Some((LEAD, 0x7fff)), 1, INDEFINITE, 1),
            (Some((1, 0)), 2, (1, 0), 2),
            (
                Some((1, 0)),
                16,
                if opcode == 0xfe {
                    (LEAD, 0x5fc2)
                } else {
                    (LEAD, 0x3fff)
                },
                if opcode == 0xfe { 0x232 } else { 0x222 },
            ),
            (
                Some((LEAD, 0x3fff)),
                32,
                if opcode == 0xfe {
                    (0xd76a_a478_4867_7021, 0x3ffe)
                } else {
                    (0x8a51_407d_a834_5c92, 0x3ffe)
                },
                0x220,
            ),
        ] {
            let mut image = stack_image(&code, 3, 0xffff);
            set_control(&mut image.cpu.x87.control, 0x037f & !mask);
            image.cpu.x87.status.precision = 0;
            if let Some(input) = input {
                write_value(&mut image.cpu, 3, input);
            }
            let mut result = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
            result.x87.status.invalid = u8::from(flags & 1 != 0);
            result.x87.status.stack_fault = u8::from(input.is_none());
            result.x87.status.denormal = u8::from(flags & 2 != 0);
            result.x87.status.underflow = u8::from(flags & 16 != 0);
            result.x87.status.precision = u8::from(flags & 32 != 0);
            result.x87.status.c1 = u8::from(flags & 0x200 != 0);
            result.x87.status.c2 = 0;
            result.x87.status.error_summary = u8::from(flags & mask != 0);
            result.x87.status.busy = result.x87.status.error_summary;
            if mask != 1 && mask != 2 {
                write_value(&mut result, 3, output);
            }
            checks.check(
                &format!("opcode={opcode:x} input={input:x?} unmasked={mask:x}"),
                &code,
                &image,
                &[dispatch(result)],
            );
        }
        let followed = [0xd9, opcode, 0xd9, 0xd0];
        let mut image = stack_image(&followed, 3, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 3, (LEAD, 0x403e));
        // A host snapshot may contain a nonempty tag inconsistent with its
        // payload. The out-of-range path does not assign ST(0), even across FNOP.
        image.cpu.x87.tag_word |= 2 << 6;
        let mut result = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
        result.x87.status.c1 = 0;
        result.x87.status.c2 = 1;
        let mut followed_result = complete_x87(result, 2, 0x01d0);
        followed_result.x87.instruction_offset = 0x1002;
        checks.check(
            "out-of-range preserves imported tag through next check",
            &followed,
            &image,
            &[dispatch(result), dispatch(followed_result)],
        );
        let mut image = stack_image(&code, 0, 0xfffc);
        image.cpu.x87.status.error_summary = 1;
        image.cpu.x87.status.busy = 1;
        checks.check(
            "pending exception preserves metadata",
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

fn specialization(engine: Engine) {
    for opcode in [0xfe, 0xff] {
        let code = [0xd9, opcode];
        let compiler = Compiler::new(SegmentProfile::Flat32);
        let compiled = compiler.compile_block(0x1000, &code, 1).unwrap();
        for input in [
            None,
            Some((1, 0)),
            Some((LEAD, 0x403e)),
            Some((LEAD, 0x7fff)),
        ] {
            let mut image = stack_image(&code, 7, 0xffff);
            image.cpu.x87.status.precision = 0;
            if let Some(input) = input {
                write_value(&mut image.cpu, 7, input);
            }
            image.check_unchanged_exit(
                engine,
                &TestModule::new(&compiled),
                "restart precedes result and C2",
                Exit::Interpret,
            );
        }
        let mut image = stack_image(&code, 7, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 7, (LEAD, 0x403e));
        let mut result = complete_x87(image.cpu, 2, 0x0100 | u16::from(opcode));
        result.x87.status.c1 = 0;
        result.x87.status.c2 = 1;
        assert_eq!(
            engine.observe(
                &TestModule::new(&compiled).with_interpreter(TestModule::interpreter()),
                &image.input(),
                1
            ),
            expected(&image, &[dispatch(result)])
        );
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
test_frontends!(values_and_reduction, values);
test_frontends!(exceptions_and_range, exceptions);
