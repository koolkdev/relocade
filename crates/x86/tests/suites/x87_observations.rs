//! Observed x87 state specializes current SSA values without replacing guest state.

#[path = "x87_observations/changes.rs"]
mod changes;
#[path = "x87_observations/precision.rs"]
mod precision;
#[path = "x87_observations/shape.rs"]
mod shape;

use crate::support::{
    machine::{expected, Exit, Step},
    step::{Engine, TestModule},
    x87::{complete_x87, dispatch, set_control, stack_image, write_value},
};
use wasm86_x86::{BlockCompiler, CpuState, SegmentProfile};

const LEADING: u64 = 1 << 63;

fn observed_cpu(pc: u8, rc: u8) -> CpuState {
    let mut observed = CpuState::default();
    // Observations and runtime backing need agree only on the logical bits.
    observed.x87.control.precision_control = pc | 0x80;
    observed.x87.control.rounding_control = rc | 0x80;
    observed
}

fn compiler(pc: u8, rc: u8) -> BlockCompiler {
    BlockCompiler::new(SegmentProfile::Flat32).specialize_on_cpu(&observed_cpu(pc, rc))
}

fn masked_precision_compiler(pc: u8, rc: u8) -> BlockCompiler {
    let mut observed = observed_cpu(pc, rc);
    observed.x87.control.precision_mask = 0x81;
    observed.x87.status.precision = 0x41;
    BlockCompiler::new(SegmentProfile::Flat32).specialize_on_cpu(&observed)
}

fn arithmetic_modes(engine: Engine) {
    for pc in 0..4 {
        let unit = match pc {
            0 => 1 << 40,
            2 => 1 << 11,
            _ => 1,
        };
        for rc in 0..4 {
            let compiler = compiler(pc, rc);
            for multiply in [false, true] {
                let opcode = if multiply { 0xc9 } else { 0xc1 };
                let code = [0xd8, opcode].repeat(8);
                let block = TestModule::new(&compiler.compile(0x1000, &code, 8).unwrap());
                for negative in [false, true] {
                    let sign = if negative { 0x8000 } else { 0 };
                    let incremented = if negative { rc == 1 } else { rc == 2 };
                    let mut image = stack_image(&code, 0, 0xfff0);
                    image.cpu.x87.status.precision = 0;
                    image.cpu.x87.control.precision_control = pc | 0xfc;
                    image.cpu.x87.control.rounding_control = rc | 0x40;
                    write_value(
                        &mut image.cpu,
                        0,
                        (LEADING + u64::from(multiply), 0x3fff | sign),
                    );
                    write_value(
                        &mut image.cpu,
                        1,
                        if multiply {
                            (LEADING + 1, 0x3fff)
                        } else {
                            (LEADING, 0x3fbf | sign) // ±2^-64: half a PC64 ulp at one.
                        },
                    );
                    // Addition repeatedly loses a half-ulp or less. Multiplying
                    // by 1+2^-63 adds one PC64 ulp plus a fraction below half;
                    // lower precisions lose that increment on every operation.
                    let significand = if multiply && unit == 1 {
                        LEADING + 1 + 8 * if incremented { 2 } else { 1 }
                    } else {
                        LEADING + if incremented { 8 * unit } else { 0 }
                    };
                    let mut result = image.cpu;
                    for _ in 0..8 {
                        result = complete_x87(result, 2, u16::from(opcode));
                    }
                    result.x87.status.precision = 1;
                    result.x87.status.c1 = u8::from(incremented);
                    write_value(&mut result, 0, (significand, 0x3fff | sign));
                    assert_eq!(
                        engine.observe(&block, &image.input(), 1),
                        expected(&image, &[dispatch(result)]),
                        "PC={pc}, RC={rc}, multiply={multiply}, negative={negative}"
                    );
                }
            }
            let code = [0xd8, 0xe1];
            let block = TestModule::new(&compiler.compile(0x1000, &code, 1).unwrap());
            let mut image = stack_image(&code, 0, 0xfff0);
            image.cpu.x87.control.precision_control = pc;
            image.cpu.x87.control.rounding_control = rc;
            write_value(&mut image.cpu, 0, (LEADING, 0x3fff));
            write_value(&mut image.cpu, 1, (LEADING, 0x3fff));
            let mut result = complete_x87(image.cpu, 2, 0x00e1);
            result.x87.status.c1 = 0;
            write_value(&mut result, 0, (0, if rc == 1 { 0x8000 } else { 0 }));
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

fn converted_stores_use_only_rounding(engine: Engine) {
    let code = [0xd9, 0x15, 0, 0x40, 0, 0, 0xdb, 0x15, 4, 0x40, 0, 0];
    for rc in 0..4 {
        let block = TestModule::new(
            &masked_precision_compiler(0, rc)
                .compile(0x1000, &code, 2)
                .unwrap(),
        );
        let mut image = stack_image(&code, 0, 0xfffc);
        image.cpu.x87.control.rounding_control = rc;
        image.cpu.x87.status.precision = 0;
        // Runtime PC64 and clear PE differ from the observations. Both stores use RC alone.
        write_value(&mut image.cpu, 0, (0xc000_0000_0000_0001, 0x3fff));
        image.map(4, 0x8000, true);
        let mut result = complete_x87(image.cpu, 6, 0x0115);
        result = complete_x87(result, 6, 0x0315);
        result.x87.data_offset = 0x4004;
        result.x87.data_selector = 0x23;
        result.x87.status.precision = 1;
        result.x87.status.c1 = u8::from(rc == 0 || rc == 2);
        let real = (0x3fc0_0000_u32 + u32::from(rc == 2)).to_le_bytes();
        let integer = if rc == 0 || rc == 2 { 2_i32 } else { 1 }.to_le_bytes();
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: result,
                    ram: &[(0x8000, &real), (0x8004, &integer)],
                    exit: Exit::Dispatch(result.eip)
                },]
            )
        );
    }
}

fn memory_arithmetic_uses_observed_modes(engine: Engine) {
    let code = [0xd8, 0x05, 0, 0x40, 0, 0]; // FADD m32
    let block = TestModule::new(
        &masked_precision_compiler(0, 2)
            .compile(0x1000, &code, 1)
            .unwrap(),
    );
    let mut image = stack_image(&code, 0, 0xfffc);
    image.cpu.x87.control.precision_control = 0;
    image.cpu.x87.control.rounding_control = 2;
    image.cpu.x87.status.precision = 0x81;
    write_value(&mut image.cpu, 0, (LEADING, 0x3fff));
    image.map(4, 0x8000, false);
    image.data(0x8000, &0x3300_0000_u32.to_le_bytes()); // 2^-25
    let mut result = complete_x87(image.cpu, 6, 0x0005);
    result.x87.data_offset = 0x4000;
    result.x87.data_selector = 0x23;
    result.x87.status.precision = 0x81;
    result.x87.status.c1 = 1;
    write_value(&mut result, 0, (LEADING + (1 << 40), 0x3fff)); // PC24: 1 + 2^-23
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

fn mismatch_restarts_at_the_consumer(engine: Engine) {
    let code = [0xb8, 7, 0, 0, 0, 0xd8, 0xc9];
    let compiled = compiler(3, 0).compile(0x1000, &code, 2).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for (pc, rc) in [(0, 0), (3, 1)] {
        let mut image = stack_image(&code, 0, 0xfff0);
        image.cpu.x87.control.precision_control = pc;
        image.cpu.x87.control.rounding_control = rc;
        write_value(&mut image.cpu, 0, (LEADING, 0x4000));
        write_value(&mut image.cpu, 1, (LEADING, 0x3fff));
        let mut restart = image.cpu;
        restart.eip += 5;
        restart.instruction_count = restart.instruction_count.wrapping_add(1);
        restart.registers.eax = 7;
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: restart,
                    ram: &[],
                    exit: Exit::Interpret
                },]
            )
        );
        let mut result = complete_x87(restart, 2, 0x00c9);
        result.x87.status.c1 = 0;
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

#[test]
fn observed_modes_preserve_arithmetic_stores_and_restart() {
    arithmetic_modes(Engine::Wasmtime);
    converted_stores_use_only_rounding(Engine::Wasmtime);
    memory_arithmetic_uses_observed_modes(Engine::Wasmtime);
    mismatch_restarts_at_the_consumer(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_observed_modes_preserve_arithmetic_stores_and_restart() {
    arithmetic_modes(Engine::V8);
    converted_stores_use_only_rounding(Engine::V8);
    memory_arithmetic_uses_observed_modes(Engine::V8);
    mismatch_restarts_at_the_consumer(Engine::V8);
}
