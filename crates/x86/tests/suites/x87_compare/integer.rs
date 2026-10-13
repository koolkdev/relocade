//! Integer sources convert exactly before the shared ordered comparison.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::compile_block_from_bytes;

fn instruction(opcode: u8, pop: bool, address: u32) -> Vec<u8> {
    [
        vec![opcode, if pop { 0x1d } else { 0x15 }],
        address.to_le_bytes().to_vec(),
    ]
    .concat()
}

fn source_bytes(opcode: u8, value: i32) -> Vec<u8> {
    if opcode == 0xde {
        (value as i16).to_le_bytes().to_vec()
    } else {
        value.to_le_bytes().to_vec()
    }
}

fn forms_and_exact_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (opcode, integer, left, relation) in [
        (0xde, -32768, (LEADING, 0xc00e), EQUAL),
        (0xde, 32767, (0xfffe_0000_0000_0000, 0x400d), EQUAL),
        (0xde, -1, (0, 0), 0),
        (0xde, 0, (0, 0x8000), EQUAL),
        (0xde, 1, (LEADING + 1, 0x3fff), 0),
        (0xde, 2, ONE, LESS),
        (0xda, i32::MIN, (LEADING, 0xc01e), EQUAL),
        (0xda, i32::MAX, (0xffff_fffe_0000_0000, 0x401d), EQUAL),
        (0xda, i32::MIN, (0, 0), 0),
        (0xda, 16_777_217, (0x8000_0080_0000_0000, 0x4017), EQUAL),
        (0xda, -1, (LEADING + 1, 0xbfff), LESS),
        (0xda, 1, (LEADING, 0x7fff), 0),
    ] {
        for pop in [false, true] {
            let bytes = source_bytes(opcode, integer);
            // The source ends at the mapping boundary; 66 cannot alter its width.
            let address = 0x5000 - bytes.len() as u32;
            let code = [vec![0x66], instruction(opcode, pop, address)].concat();
            let mut image = initial_image(&code);
            write_value(&mut image.cpu, 7, left);
            set_control(&mut image.cpu.x87.control, 0x0c7f); // PC24, round toward zero
            image.map(4, 0x8000, false);
            image.data(address + 0x4000, &bytes);
            let saved_opcode = (u16::from(opcode & 7) << 8) | u16::from(code[2]);
            let mut result = completed(image.cpu, 7, saved_opcode, relation, u8::from(pop));
            result.x87.data_offset = address;
            result.x87.data_selector = 0x23;
            checks.check(
                "signed source and exact extended ordering",
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

fn operand_exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for opcode in [0xde, 0xda] {
        for (left, empty, exception, relation) in [
            (QNAN, false, 1, UNORDERED),
            (SNAN, false, 1, UNORDERED),
            ((1, 0x3fff), false, 1, UNORDERED),
            ((1, 0), false, 2, LESS),
            ((LEADING, 0), false, 2, LESS),
            ((1, 0), true, 0x41, UNORDERED),
        ] {
            for masked in [false, true] {
                let code = [instruction(opcode, true, 0x4000), vec![0x9b]].concat();
                let mut image = initial_image(&code);
                write_value(&mut image.cpu, 7, left);
                if empty {
                    image.cpu.x87.tag_word |= 0xc000;
                }
                set_control(
                    &mut image.cpu.x87.control,
                    if masked { 0x037f } else { 0x037c },
                );
                image.map(4, 0x8000, false);
                image.data(0x8000, &source_bytes(opcode, 1));
                let flags = exception | if masked { relation } else { 0x4100 | PENDING };
                let mut result = completed(
                    image.cpu,
                    6,
                    (u16::from(opcode & 7) << 8) | 0x1d,
                    flags,
                    u8::from(masked),
                );
                result.x87.data_offset = 0x4000;
                result.x87.data_selector = 0x23;
                let mut waited = result;
                let exit = if masked {
                    waited.eip += 1;
                    waited.instruction_count = waited.instruction_count.wrapping_add(1);
                    Exit::Dispatch(waited.eip)
                } else {
                    Exit::FloatingPoint
                };
                checks.check(
                    "only ST0 supplies numerical exception evidence",
                    &code,
                    &image,
                    &[
                        dispatch(result),
                        Step {
                            cpu: waited,
                            ram: &[],
                            exit,
                        },
                    ],
                );
            }
        }
    }
}

fn memory_faults(engine: Engine, frontend: Frontend) {
    let code = instruction(0xda, true, 0x4ffe);
    let mut image = initial_image(&code);
    write_value(&mut image.cpu, 7, QNAN);
    image.cpu.x87.tag_word |= 0xc000;
    set_control(&mut image.cpu.x87.control, 0x037e);
    image.map(4, 0x8000, false);
    image.data(0x8ffe, &[0, 0]);
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    checks.check(
        "complete integer span faults before comparison effects",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        }],
    );
    image.cpu.x87.status.invalid = 1;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "pending exception precedes integer access",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );

    let mut image = initial_image(&code);
    image.map(4, 0x8000, false);
    image.map(5, 0xa000, false);
    image.data(0x8ffe, &[0, 0]);
    image.data(0xa000, &[0, 0x80]); // i32::MIN across two physical pages
    write_value(&mut image.cpu, 7, (LEADING, 0xc01e));
    let mut result = completed(image.cpu, 6, 0x021d, EQUAL, 1);
    result.x87.data_offset = 0x4ffe;
    result.x87.data_selector = 0x23;
    checks.check(
        "split integer source is assembled before sign extension",
        &code,
        &image,
        &[dispatch(result)],
    );
}

fn compiled_continuation_and_restart(engine: Engine) {
    // FICOMP's pop and saved memory pointer feed a following EFLAGS comparison.
    let code = [instruction(0xda, true, 0x4000), vec![0xdb, 0xf1]].concat();
    let mut image = initial_image(&code);
    image.cpu.flags.status_source.kind = 0;
    set_control(&mut image.cpu.x87.control, 0x0c7f);
    image.map(4, 0x8000, false);
    image.data(0x8000, &16_777_217_i32.to_le_bytes());
    write_value(&mut image.cpu, 7, (0x8000_0080_0000_0000, 0x4017));
    write_value(&mut image.cpu, 0, ONE);
    write_value(&mut image.cpu, 1, (LEADING, 0x4000));
    let mut first = completed(image.cpu, 6, 0x021d, EQUAL, 1);
    first.x87.data_offset = 0x4000;
    first.x87.data_selector = 0x23;
    let mut final_cpu = complete_x87(first, 2, 0x03f1);
    final_cpu.flags.status_source.kind = 0;
    final_cpu.flags.bytes.cf = 1;
    final_cpu.flags.bytes.pf = 0;
    final_cpu.flags.bytes.af = 0;
    final_cpu.flags.bytes.zf = 0;
    final_cpu.flags.bytes.sf = 0;
    final_cpu.flags.bytes.of = 0;
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 2).unwrap());
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(&image, &[dispatch(final_cpu)])
    );
    assert_eq!(
        engine.observe(TestModule::interpreter(), &image.input(), 2),
        expected(&image, &[dispatch(first), dispatch(final_cpu)])
    );

    // FCMOV copies a signaling NaN exactly; the integer comparison then restarts
    // before overwriting the move's metadata or acquiring its memory pointer.
    let code = [vec![0xda, 0xc1], instruction(0xde, true, 0x4000)].concat();
    let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for masked in [false, true] {
        let mut image = initial_image(&code);
        image.cpu.flags.status_source.kind = 0;
        image.cpu.flags.bytes.cf = 1;
        write_value(&mut image.cpu, 7, ONE);
        write_value(&mut image.cpu, 0, SNAN);
        set_control(
            &mut image.cpu.x87.control,
            if masked { 0x037f } else { 0x037e },
        );
        image.map(4, 0x8000, false);
        image.data(0x8000, &1_i16.to_le_bytes());
        let mut prefix = complete_x87(image.cpu, 2, 0x02c1);
        write_value(&mut prefix, 7, SNAN);
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
        let mut result = completed(
            prefix,
            6,
            0x061d,
            if masked {
                UNORDERED | 1
            } else {
                0x4101 | PENDING
            },
            u8::from(masked),
        );
        result.x87.data_offset = 0x4000;
        result.x87.data_selector = 0x23;
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }
}

test_frontends!(forms, forms_and_exact_values);
test_frontends!(exceptions, operand_exceptions);
test_frontends!(memory, memory_faults);
#[test]
fn compiled_sequence_and_restart() {
    compiled_continuation_and_restart(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_compiled_sequence_and_restart() {
    compiled_continuation_and_restart(Engine::V8);
}

#[test]
fn complete_encodings() {
    for opcode in [0xde, 0xda] {
        for pop in [false, true] {
            crate::support::encoding::check_length(&instruction(opcode, pop, 0x4000));
        }
    }
}
