//! Restart guards must not repeat an observable operand read.

use super::*;
use crate::support::{
    machine::expected,
    step::{Event, MmioUpdate, TestModule},
};
use wasm86_x86::Compiler;

fn x87_image(code: &[u8]) -> Image {
    let mut image = image(code);
    image.cpu.x87 = Default::default();
    image.cpu.x87.registers[0].significand = 1 << 63;
    image.cpu.x87.registers[0].sign_exponent = 0x3fff;
    image
}

fn memory_result(image: &Image, opcode: u8, modrm: u8) -> CpuState {
    let mut cpu = retired(image, 4);
    cpu.x87.opcode = (u16::from(opcode & 7) << 8) | u16::from(modrm);
    cpu.x87.instruction_offset = 0x1000;
    cpu.x87.instruction_selector = 0;
    cpu.x87.data_offset = 0x2000;
    cpu.x87.data_selector = 0;
    cpu
}

fn check_read(
    engine: Engine,
    module: &TestModule,
    image: &Image,
    cpu: CpuState,
    bytes: u32,
    exit: Exit,
) {
    let replacement = 1.0_f32.to_bits().to_le_bytes();
    let mut input = image.input();
    input.mmio_pages = vec![(2, 0x8000)];
    input.observe_mmio = true;
    input.mmio_updates = vec![MmioUpdate {
        guest: vec![(0x8000, replacement.to_vec())],
        ..MmioUpdate::default()
    }];
    let mut wanted = expected(
        image,
        &[Step {
            cpu,
            ram: &[(0x8000, &replacement)],
            exit,
        }],
    );
    wanted.events.insert(
        0,
        Event::MmioRead {
            address: 0x2000,
            bytes,
        },
    );
    assert_eq!(engine.observe(module, &input, 1), wanted);
}

fn exceptional_operands(engine: Engine, frontend: Frontend) {
    let interpreter = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    for (opcode, modrm) in [(0xd9, 0x06), (0xdf, 0x06), (0xd8, 0x16), (0xd8, 0x0e)] {
        let code = [opcode, modrm, 0, 0x20]; // FLD, FILD, FCOM, FMUL [2000]
        let mut image = x87_image(&code);
        image.cpu.x87.tag_word = match opcode {
            0xd9 => 0xffff, // FLD into an empty stack.
            0xdf => 0,      // FILD into a full stack.
            _ => 0xfffc,    // FCOM/FMUL with ST(0) = 1.
        };
        image.data(0x8000, &0x7f80_0001_u32.to_le_bytes()); // Positive SNaN.
        let mut cpu = memory_result(&image, opcode, modrm);
        cpu.x87.status.invalid = 1;
        match (opcode, modrm) {
            (0xd9, _) => {
                cpu.x87.status.top = 7;
                cpu.x87.tag_word = 0xbfff;
                cpu.x87.registers[7].significand = 0xc000_0100_0000_0000;
                cpu.x87.registers[7].sign_exponent = 0x7fff;
            }
            (0xdf, _) => {
                cpu.x87.status.top = 7;
                cpu.x87.status.stack_fault = 1;
                cpu.x87.status.c1 = 1;
                cpu.x87.tag_word = 0x8000;
                cpu.x87.registers[7].significand = 0xc000_0000_0000_0000;
                cpu.x87.registers[7].sign_exponent = 0xffff;
            }
            (_, 0x16) => {
                cpu.x87.status.c0 = 1;
                cpu.x87.status.c2 = 1;
                cpu.x87.status.c3 = 1;
            }
            _ => {
                cpu.x87.tag_word = 0xfffe;
                cpu.x87.registers[0].significand = 0xc000_0100_0000_0000;
                cpu.x87.registers[0].sign_exponent = 0x7fff;
            }
        }
        let block = matches!(frontend, Frontend::Block).then(|| {
            TestModule::new(
                &Compiler::new(ExecutionProfile::Real16)
                    .compile_block(0x1000, &code, 1)
                    .unwrap(),
            )
            .with_interpreter(interpreter)
        });
        check_read(
            engine,
            block.as_ref().unwrap_or(interpreter),
            &image,
            cpu,
            if opcode == 0xdf { 2 } else { 4 },
            Exit::Dispatch(cpu.eip),
        );
    }
}
test_frontends!(exceptional_x87_operands_are_read_once, exceptional_operands);

fn changed_control(engine: Engine, frontend: Frontend) {
    let code = [0xd8, 0x0e, 0, 0x20]; // FMUL [2000]
    let mut image = x87_image(&code);
    image.cpu.x87.tag_word = 0xfffc;
    image.cpu.x87.control.rounding_control = 1;
    image.data(0x8000, &2.0_f32.to_bits().to_le_bytes());
    let mut observed = image.cpu;
    observed.x87.control.rounding_control = 0;
    let interpreter = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    let block = matches!(frontend, Frontend::Block).then(|| {
        TestModule::new(
            &Compiler::new(ExecutionProfile::Real16)
                .specialize_on_cpu(&observed)
                .compile_block(0x1000, &code, 1)
                .unwrap(),
        )
        .with_interpreter(interpreter)
    });
    let mut cpu = memory_result(&image, code[0], code[1]);
    cpu.x87.registers[0].sign_exponent = 0x4000; // 1 * first device value (2).
    check_read(
        engine,
        block.as_ref().unwrap_or(interpreter),
        &image,
        cpu,
        4,
        Exit::Dispatch(cpu.eip),
    );
}
test_frontends!(changed_x87_controls_do_not_repeat_mmio, changed_control);

fn next_instruction(engine: Engine) {
    let code = [0xd9, 0x06, 0, 0x20, 0xd9, 0xe8]; // FLD [2000]; FLD1
    let mut image = x87_image(&code);
    image.cpu.x87.tag_word = 0xc000; // Only the first push fits.
    image.data(0x8000, &1.0_f32.to_bits().to_le_bytes());
    let block = TestModule::new(
        &Compiler::new(ExecutionProfile::Real16)
            .compile_block(0x1000, &code, 2)
            .unwrap(),
    );
    let mut cpu = memory_result(&image, code[0], code[1]);
    cpu.x87.status.top = 7;
    cpu.x87.tag_word = 0;
    cpu.x87.registers[7].significand = 1 << 63;
    cpu.x87.registers[7].sign_exponent = 0x3fff;
    check_read(engine, &block, &image, cpu, 4, Exit::Interpret);
}

#[test]
fn next_instruction_can_specialize_after_mmio() {
    next_instruction(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_next_instruction_can_specialize_after_mmio() {
    next_instruction(Engine::V8);
}
