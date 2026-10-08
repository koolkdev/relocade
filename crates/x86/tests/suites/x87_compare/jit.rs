//! Native ordering retains exact values; operand exceptions restart before effects.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::compile_block_from_bytes;

fn loaded(mut cpu: CpuState, address: u32, value: (u64, u16)) -> CpuState {
    cpu = complete_x87(cpu, 6, 0x0505);
    cpu.x87.status.top = cpu.x87.status.top.wrapping_sub(1) & 7;
    cpu.x87.status.c1 = 0;
    cpu.x87.data_offset = address;
    cpu.x87.data_selector = 0x23;
    let top = cpu.x87.status.top;
    write_value(&mut cpu, usize::from(top), value);
    cpu
}

fn native_values(engine: Engine) {
    // Each native encoding has an independently specified extended snapshot.
    let values = [
        (0x3ff0_0000_0000_0000_u64, ONE),
        (0x3ff0_0000_0000_0001, (LEADING + 0x800, 0x3fff)),
        (0xbff0_0000_0000_0000, (LEADING, 0xbfff)),
        (0xbff0_0000_0000_0001, (LEADING + 0x800, 0xbfff)),
        (0, (0, 0)),
        (0x8000_0000_0000_0000, (0, 0x8000)),
        (0x0010_0000_0000_0000, (LEADING, 0x3c01)),
        (0x7fef_ffff_ffff_ffff, (0xffff_ffff_ffff_f800, 0x43fe)),
        (0x7ff0_0000_0000_0000, (LEADING, 0x7fff)),
        (0xfff0_0000_0000_0000, (LEADING, 0xffff)),
        (0x7ff8_0000_0000_0001, (0xc000_0000_0000_0800, 0x7fff)),
    ];
    for memory in [true, false] {
        let code: &[u8] = if memory {
            // FLD m64; FCOMP m64
            &[0xdd, 0x05, 0, 0x40, 0, 0, 0xdc, 0x1d, 8, 0x40, 0, 0]
        } else {
            // Load the right operand first, then the left; FUCOMPP.
            &[
                0xdd, 0x05, 8, 0x40, 0, 0, 0xdd, 0x05, 0, 0x40, 0, 0, 0xda, 0xe9,
            ]
        };
        let count = if memory { 2 } else { 3 };
        let block = TestModule::new(&compile_block_from_bytes(0x1000, code, count).unwrap());
        for (left, right, flags) in [
            (0, 1, LESS),
            (1, 0, 0),
            (0, 0, EQUAL),
            (2, 3, 0),
            (3, 2, LESS),
            (4, 5, EQUAL),
            (5, 4, EQUAL),
            (6, 7, LESS),
            (7, 8, LESS),
            (9, 2, LESS),
            (8, 8, EQUAL),
            (10, 0, UNORDERED),
            (0, 10, UNORDERED),
        ] {
            if memory && flags == UNORDERED {
                // Ordered NaNs take the operand-exception restart tested below.
                continue;
            }
            let mut image = stack_image(code, 0, 0xffff);
            image.cpu.x87.status = status(0x20);
            // Comparing does not require a precision, rounding or mask assumption.
            set_control(&mut image.cpu.x87.control, 0x0c40);
            image.map(4, 0x8000, false);
            image.data(0x8000, &values[left].0.to_le_bytes());
            image.data(0x8008, &values[right].0.to_le_bytes());
            let mut steps = Vec::new();
            let mut cpu = image.cpu;
            if !memory {
                cpu = loaded(cpu, 0x4008, values[right].1);
                steps.push(dispatch(cpu));
            }
            cpu = loaded(cpu, 0x4000, values[left].1);
            steps.push(dispatch(cpu));
            cpu = completed(
                cpu,
                if memory { 6 } else { 2 },
                if memory { 0x041d } else { 0x02e9 },
                flags,
                if memory { 1 } else { 2 },
            );
            if memory {
                cpu.x87.data_offset = 0x4008;
            }
            steps.push(dispatch(cpu));
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(&image, &[dispatch(cpu)]),
                "native comparison stays compiled: memory={memory}, {left} versus {right}"
            );
            assert_eq!(
                engine.observe(TestModule::interpreter(), &image.input(), count as usize),
                expected(&image, &steps)
            );
        }
    }
}

fn restart_after_load(engine: Engine) {
    let code = [0xdd, 0x05, 0, 0x40, 0, 0, 0xdc, 0x1d, 8, 0x40, 0, 0];
    let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for (source, exception, relation) in [
        (0x7ff8_0000_0000_0001_u64, 1, UNORDERED),
        (0x7ff0_0000_0000_0001, 1, UNORDERED),
        (1, 2, 0),
    ] {
        for masked in [false, true] {
            let mut image = stack_image(&code, 0, 0xffff);
            image.cpu.x87.status = status(0x4320);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037f & !exception },
            );
            image.map(4, 0x8000, false);
            image.data(0x8000, &1.0_f64.to_bits().to_le_bytes());
            image.data(0x8008, &source.to_le_bytes());
            let first = loaded(image.cpu, 0x4000, ONE);
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: first,
                        ram: &[],
                        exit: Exit::Interpret,
                    }]
                ),
                "restart retains the preceding load and its saved pointers"
            );
            let flags = exception | if masked { relation } else { 0x4100 | PENDING };
            let mut result = completed(first, 6, 0x041d, flags, u8::from(masked));
            result.x87.data_offset = 0x4008;
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(result)]),
                "interpreter completes the excluded operand response"
            );
        }
    }
}

fn wide_values_stay_compiled(engine: Engine) {
    for (code, left, right, flags) in [
        ([0xd8, 0xd1], (LEADING + 1, 0x3fff), ONE, 0),
        ([0xd8, 0xd1], (LEADING, 1), (LEADING, 0x7ffe), LESS),
        ([0xdd, 0xe1], QNAN, (1, 0), UNORDERED),
        ([0xdd, 0xe1], (LEADING, 0x7fff), ONE, 0),
    ] {
        let mut image = initial_image(&code);
        set_control(&mut image.cpu.x87.control, 0x0c40);
        write_value(&mut image.cpu, 7, left);
        write_value(&mut image.cpu, 0, right);
        let opcode = (u16::from(code[0] & 7) << 8) | u16::from(code[1]);
        let result = completed(image.cpu, 2, opcode, flags, 0);
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

#[test]
fn native_and_extended_ordering() {
    native_values(Engine::Wasmtime);
    wide_values_stay_compiled(Engine::Wasmtime);
}

#[test]
fn operand_exception_restart() {
    restart_after_load(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_and_extended_ordering() {
    native_values(Engine::V8);
    wide_values_stay_compiled(Engine::V8);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_operand_exception_restart() {
    restart_after_load(Engine::V8);
}
