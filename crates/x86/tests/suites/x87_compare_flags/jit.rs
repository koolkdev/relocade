//! Native comparisons and exact restart preserve the preceding flag producer.

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
    let code = [
        0xdd, 0x05, 8, 0x40, 0, 0, 0xdd, 0x05, 0, 0x40, 0, 0, 0xdf, 0xe9,
    ];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 3).unwrap());
    for (bits, value, flags) in [
        (0x3ff0_0000_0000_0000_u64, ONE, 0x40),
        (0x3ff0_0000_0000_0001, (LEADING + 0x800, 0x3fff), 0),
        (0xbff0_0000_0000_0000, (LEADING, 0xbfff), 1),
        (0x0010_0000_0000_0000, (LEADING, 0x3c01), 1),
        (0x7ff0_0000_0000_0000, (LEADING, 0x7fff), 0),
        (0x7ff8_0000_0000_0001, (0xc000_0000_0000_0800, 0x7fff), 0x45),
    ] {
        let mut image = initial_image(&code);
        image.cpu.x87.tag_word = 0xffff;
        set_control(&mut image.cpu.x87.control, 0x0c40);
        image.map(4, 0x8000, false);
        image.data(0x8000, &bits.to_le_bytes());
        image.data(0x8008, &1.0_f64.to_bits().to_le_bytes());
        let right = loaded(image.cpu, 0x4008, ONE);
        let left = loaded(right, 0x4000, value);
        let result = completed(left, 0x07e9, flags, 0, 1);
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
        assert_eq!(
            engine.observe(TestModule::interpreter(), &image.input(), 3),
            expected(&image, &[dispatch(right), dispatch(left), dispatch(result)])
        );
    }
    // Full extended range and precision remain compiled without narrowing.
    for (left, right, flags) in [
        ((LEADING + 1, 0x3fff), ONE, 0),
        ((LEADING, 1), (LEADING, 0x7ffe), 1),
        (QNAN, (1, 0), 0x45),
        ((0, 0x8000), (0, 0), 0x40),
    ] {
        let code = [0xdb, 0xe9];
        let mut image = initial_image(&code);
        write_value(&mut image.cpu, 7, left);
        write_value(&mut image.cpu, 0, right);
        set_control(&mut image.cpu.x87.control, 0x0c40);
        let result = completed(image.cpu, 0x03e9, flags, 0, 0);
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

fn restart_after_integer_compare(engine: Engine) {
    let code = [0x39, 0xc8, 0xdf, 0xf1]; // CMP EAX, ECX; FCOMIP ST1
    let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for (source, exception, relation) in [(QNAN, 1, 0x45), ((1, 0), 2, 0)] {
        for masked in [false, true] {
            let mut image = initial_image(&code);
            image.cpu.registers.eax = 0;
            image.cpu.registers.ecx = 1;
            write_value(&mut image.cpu, 0, source);
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037c },
            );
            let mut prefix = image.cpu;
            prefix.eip += 2;
            prefix.instruction_count = prefix.instruction_count.wrapping_add(1);
            prefix.flags.status_source.kind = 9;
            prefix.flags.status_source.left = 0;
            prefix.flags.status_source.right = 1;
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: prefix,
                        ram: &[],
                        exit: Exit::Interpret
                    }]
                )
            );
            let result = completed(
                prefix,
                0x07f1,
                if masked { relation } else { 5 },
                exception | if masked { 0 } else { 0x8080 },
                u8::from(masked),
            );
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

#[test]
fn native_and_extended_ordering() {
    native_values(Engine::Wasmtime);
}
#[test]
fn operand_exception_restart() {
    restart_after_integer_compare(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_and_extended_ordering() {
    native_values(Engine::V8);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_operand_exception_restart() {
    restart_after_integer_compare(Engine::V8);
}
