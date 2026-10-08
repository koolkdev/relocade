//! Value selection survives subsequent stack reads and consumes live flag sources.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::compile_block_from_bytes;

fn stack_continuation(engine: Engine, frontend: Frontend) {
    let code = [0xda, 0xc1, 0xdd, 0xd2]; // FCMOVB ST1; FST ST2
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for taken in [false, true] {
        let mut image = initial_image(&code);
        image.cpu.flags.bytes.cf = u8::from(taken);
        let value = if taken { TWO } else { ONE };
        let mut moved = complete_x87(image.cpu, 2, 0x02c1);
        write_value(&mut moved, 7, value);
        let mut stored = complete_x87(moved, 2, 0x05d2);
        stored.x87.status.c1 = 0;
        write_value(&mut stored, 1, value);
        checks.check(
            "next exception check retains the conditional value",
            &code,
            &image,
            &[dispatch(moved), dispatch(stored)],
        );
    }
}

fn live_integer_flags(engine: Engine, frontend: Frontend) {
    let code = [0x39, 0xc8, 0xda, 0xd1]; // CMP EAX, ECX; FCMOVBE ST1
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, right, taken) in [(0, 1, true), (1, 1, true), (2, 1, false)] {
        let mut image = initial_image(&code);
        image.cpu.registers.eax = left;
        image.cpu.registers.ecx = right;
        let mut compared = image.cpu;
        compared.eip += 2;
        compared.instruction_count = compared.instruction_count.wrapping_add(1);
        compared.flags.status_source.kind = 9;
        compared.flags.status_source.left = left;
        compared.flags.status_source.right = right;
        let mut moved = complete_x87(compared, 2, 0x02d1);
        if taken {
            write_value(&mut moved, 7, TWO);
        }
        checks.check(
            "integer compare feeds the move without materializing flags",
            &code,
            &image,
            &[dispatch(compared), dispatch(moved)],
        );
    }
}

fn native_maximum(engine: Engine) {
    // Retained binary64 values survive conditional selection and conversion back.
    let code = [
        0xdd, 0x05, 8, 0x40, 0, 0, 0xdd, 0x05, 0, 0x40, 0, 0, 0xdb, 0xe9, 0xda, 0xc1, 0xdd, 0x1d,
        16, 0x40, 0, 0,
    ];
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 5).unwrap());
    for (left_bits, left, right_bits, right, flags, maximum) in [
        (
            1.0_f64.to_bits(),
            ONE,
            2.0_f64.to_bits(),
            TWO,
            1,
            2.0_f64.to_bits(),
        ),
        (
            2.0_f64.to_bits(),
            TWO,
            1.0_f64.to_bits(),
            ONE,
            0,
            2.0_f64.to_bits(),
        ),
        (
            1.0_f64.to_bits(),
            ONE,
            1.0_f64.to_bits(),
            ONE,
            0x40,
            1.0_f64.to_bits(),
        ),
    ] {
        let mut image = initial_image(&code);
        image.cpu.x87.tag_word = 0xffff;
        image.map(4, 0x8000, true);
        image.data(0x8000, &left_bits.to_le_bytes());
        image.data(0x8008, &right_bits.to_le_bytes());
        let mut cpu = image.cpu;
        let mut steps = Vec::new();
        for (address, value) in [(0x4008, right), (0x4000, left)] {
            cpu = complete_x87(cpu, 6, 0x0505);
            cpu.x87.status.top = cpu.x87.status.top.wrapping_sub(1) & 7;
            cpu.x87.status.c1 = 0;
            cpu.x87.data_offset = address;
            cpu.x87.data_selector = 0x23;
            let top = usize::from(cpu.x87.status.top);
            write_value(&mut cpu, top, value);
            steps.push(dispatch(cpu));
        }
        cpu = complete_x87(cpu, 2, 0x03e9);
        cpu.flags.status_source.kind = 0;
        cpu.flags.bytes.cf = flags & 1;
        cpu.flags.bytes.pf = 0;
        cpu.flags.bytes.zf = (flags >> 6) & 1;
        cpu.flags.bytes.of = 0;
        cpu.flags.bytes.sf = 0;
        cpu.flags.bytes.af = 0;
        steps.push(dispatch(cpu));
        cpu = complete_x87(cpu, 2, 0x02c1);
        if flags & 1 != 0 {
            write_value(&mut cpu, 5, right);
        }
        steps.push(dispatch(cpu));
        cpu = complete_x87(cpu, 6, 0x051d);
        cpu.x87.status.top = 6;
        cpu.x87.tag_word |= 3 << 10;
        cpu.x87.data_offset = 0x4010;
        let output = maximum.to_le_bytes();
        let ram = [(0x8010, output.as_slice())];
        steps.push(Step {
            cpu,
            ram: &ram,
            exit: Exit::Dispatch(cpu.eip),
        });
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(&image, &steps[4..])
        );
        assert_eq!(
            engine.observe(TestModule::interpreter(), &image.input(), 5),
            expected(&image, &steps)
        );
        assert_eq!(
            register_bits(&cpu, 5),
            if flags & 1 != 0 { right } else { left }
        );
    }
}

test_frontends!(continuation, stack_continuation);
test_frontends!(integer_flags, live_integer_flags);
#[test]
fn native_compare_move_store() {
    native_maximum(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_native_compare_move_store() {
    native_maximum(Engine::V8);
}

fn restart_after_integer_compare(engine: Engine) {
    let code = [0x39, 0xc8, 0xda, 0xc1];
    let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for empty in [1, 2] {
        for masked in [false, true] {
            let mut image = initial_image(&code);
            image.cpu.registers.eax = 2;
            image.cpu.registers.ecx = 1;
            set_control(
                &mut image.cpu.x87.control,
                if masked { 0x037f } else { 0x037e },
            );
            image.cpu.x87.tag_word |= if empty == 1 { 0xc000 } else { 3 };
            let mut prefix = image.cpu;
            prefix.eip += 2;
            prefix.instruction_count = prefix.instruction_count.wrapping_add(1);
            prefix.flags.status_source.kind = 9;
            prefix.flags.status_source.left = 2;
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
            let mut result = complete_x87(prefix, 2, 0x02c1);
            result.x87.status.invalid = 1;
            result.x87.status.stack_fault = 1;
            result.x87.status.c1 = 0;
            if masked {
                write_value(&mut result, 7, INDEFINITE);
            } else {
                result.x87.status.error_summary = 1;
                result.x87.status.busy = 1;
            }
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(result)])
            );
        }
    }
}

#[test]
fn empty_operand_restart() {
    restart_after_integer_compare(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_empty_operand_restart() {
    restart_after_integer_compare(Engine::V8);
}
