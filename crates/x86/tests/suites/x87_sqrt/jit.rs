//! Native roots retain their views and rejected operands restart before effects.

use super::*;
use crate::support::wasm::operators;
use crate::support::{execution::CompiledExecution, machine::expected, step::TestModule};
use relocade_x86::{CompiledModule, Compiler, CpuState};
use wasmparser::Operator;

fn native_shape(compiled: &CompiledModule) {
    let mut roots = 0;
    for op in operators(&compiled.bytes) {
        match op {
            Operator::F64Sqrt => roots += 1,
            Operator::Loop { .. } => panic!("native root emitted an integer refinement loop"),
            _ => {}
        }
    }
    assert_eq!(roots, 1);
}

fn native_values() {
    let code = [
        0xdd, 0x05, 0, 0x40, 0, 0, // FLD m64
        0xd9, 0xfa, // FSQRT
        0xdd, 0x15, 8, 0x40, 0, 0, // FST m64
    ];
    let mut observed = CpuState::default();
    set_control(&mut observed.x87.control, 0x027f);
    for instructions in [2, 3] {
        let compiled = Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&observed)
            .compile_block(0x1000, &code, instructions)
            .unwrap();
        native_shape(&compiled);
        let module = TestModule::new(&compiled);
        for input in [
            0.0_f64,
            -0.0,
            1.0,
            2.0,
            4.0,
            9.0,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::from_bits(1.0_f64.to_bits() + 1),
            f64::from_bits(2.0_f64.to_bits() + 1),
        ] {
            let bits = input.to_bits();
            let extended = if input == 0.0 {
                (0, (bits >> 48) as u16)
            } else {
                (
                    LEAD | (bits << 11),
                    ((bits >> 52) as u16 & 0x7ff) + 16383 - 1023,
                )
            };
            let (root, inexact, increment) = expected_root(extended, 2, 0);
            let root_bits = if root.0 == 0 {
                u64::from(root.1) << 48
            } else {
                (u64::from(root.1 + 1023 - 16383) << 52) | ((root.0 >> 11) & ((1 << 52) - 1))
            };
            let mut image = stack_image(&code, 0, 0xffff);
            image.cpu.x87.control = observed.x87.control;
            image.cpu.x87.status.precision = 0;
            image.map(4, 0x8000, true);
            image.data(0x8000, &input.to_le_bytes());
            let mut result = complete_x87(image.cpu, 6, 0x0505);
            result = complete_x87(result, 2, 0x01fa);
            result.x87.status.top = 7;
            result.x87.status.precision = u8::from(inexact);
            result.x87.status.c1 = u8::from(increment);
            result.x87.data_offset = 0x4000;
            result.x87.data_selector = 0x23;
            write_value(&mut result, 7, root);
            let bytes = root_bits.to_le_bytes();
            let writes = [(0x8008, bytes.as_slice())];
            if instructions == 3 {
                result = complete_x87(result, 6, 0x0515);
                result.x87.status.c1 = 0;
                result.x87.data_offset = 0x4008;
            }
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: result,
                        ram: if instructions == 3 { &writes } else { &[] },
                        exit: Exit::Dispatch(result.eip),
                    }]
                ),
                "input={input}, instructions={instructions}"
            );
        }
    }
}

fn extended_native_range() {
    let code = [0xd9, 0xe8, 0xd9, 0xfd, 0xd9, 0xfa, 0xd8, 0xc0];
    let mut observed = CpuState::default();
    set_control(&mut observed.x87.control, 0x027f);
    for instructions in [3, 4] {
        let compiled = Compiler::new(SegmentProfile::Flat32)
            .specialize_on_cpu(&observed)
            .compile_block(0x1000, &code, instructions)
            .unwrap();
        native_shape(&compiled);
        for scale in [-4001_i32, -4000, 4000, 4001] {
            let mut image = stack_image(&code, 0, 0xffff);
            image.cpu.x87.control = observed.x87.control;
            image.cpu.x87.status.precision = 0;
            write_value(
                &mut image.cpu,
                0,
                (
                    u64::from(scale.unsigned_abs()) << 52,
                    0x400a | if scale < 0 { 0x8000 } else { 0 },
                ),
            );
            let (mut root, inexact, increment) =
                expected_root((LEAD, (16383 + scale) as u16), 2, 0);
            let mut result = complete_x87(image.cpu, 2, 0x01e8);
            result = complete_x87(result, 2, 0x01fd);
            result = complete_x87(result, 2, 0x01fa);
            result.x87.status.top = 7;
            result.x87.status.precision = u8::from(inexact);
            result.x87.status.c1 = u8::from(increment);
            if instructions == 4 {
                result = complete_x87(result, 2, 0x00c0);
                result.x87.status.c1 = 0;
                root.1 += 1;
            }
            write_value(&mut result, 7, root);
            assert_eq!(
                TestModule::new(&compiled).observe(&image.input(), 1),
                expected(&image, &[dispatch(result)]),
                "scale={scale}, instructions={instructions}"
            );
        }
    }
}

fn restart() {
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &CODE, 1)
        .unwrap();
    let checks = CompiledExecution::new(&compiled);
    for (input, output, invalid, denormal) in [
        (Some((LEAD, 0xbfff)), INDEFINITE, 1, 0),
        (Some((2, 0)), (LEAD, 0x1fe1), 0, 1),
        (
            Some((LEAD + 1, 0xffff)),
            (0xc000_0000_0000_0001, 0xffff),
            1,
            0,
        ),
        (None, INDEFINITE, 1, 0),
    ] {
        let mut image = stack_image(&CODE, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        if let Some(input) = input {
            write_value(&mut image.cpu, 0, input);
        }
        let mut result = complete_x87(image.cpu, 2, 0x01fa);
        result.x87.status.c1 = 0;
        result.x87.status.invalid = invalid;
        result.x87.status.denormal = denormal;
        result.x87.status.stack_fault = u8::from(input.is_none());
        write_value(&mut result, 0, output);
        checks.check_restart(
            "FSQRT restarts before effects",
            &image,
            &[],
            &dispatch(result),
        );
    }
    // An unobserved block admits all PC/RC modes and full-width normal operands.
    for pc in 0..4 {
        for rc in 0..4 {
            let mut image = stack_image(&CODE, 0, 0xffff);
            image.cpu.x87.status.precision = 0;
            set_control(&mut image.cpu.x87.control, 0x007f | (pc << 8) | (rc << 10));
            let input = (0xdcce_60bd_0e0a_87ff, 0x4000);
            write_value(&mut image.cpu, 0, input);
            let (output, inexact, increment) = expected_root(input, pc, rc);
            let mut result = complete_x87(image.cpu, 2, 0x01fa);
            result.x87.status.precision = u8::from(inexact);
            result.x87.status.c1 = u8::from(increment);
            write_value(&mut result, 0, output);
            checks.check_native("FSQRT admitted operand", &image, &dispatch(result));
        }
    }
}

#[test]
fn native_views_and_full_precision_restart() {
    native_values();
    extended_native_range();
    restart();
}
