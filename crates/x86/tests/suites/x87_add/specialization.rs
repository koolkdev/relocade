//! Normal and zero sums remain live; exceptional sums restart before effects.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::compile_block_from_bytes;

fn live_arithmetic(engine: Engine) {
    // A multiplication feeds cancellation, then addition feeds multiplication.
    // This checks the zero/normal facts across the shared numerical interface.
    let code = [0xd8, 0xc9, 0xd8, 0xe2, 0xd8, 0xc1, 0xd8, 0xc9];
    let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 4).unwrap());
    let mut image = stack_image(&code, 0, 0xffc0);
    write_value(&mut image.cpu, 0, (LEADING, 0x4000)); // 2
    write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x4000)); // 3
    write_value(&mut image.cpu, 2, (0xc000_0000_0000_0000, 0x4001)); // 6
    let mut result = image.cpu;
    for (opcode, bits) in [
        (0x00c9, (0xc000_0000_0000_0000, 0x4001)),
        (0x00e2, (0, 0)),
        (0x00c1, (0xc000_0000_0000_0000, 0x4000)),
        (0x00c9, (0x9000_0000_0000_0000, 0x4002)),
    ] {
        result = complete_x87(result, 2, opcode);
        result.x87.status.c1 = 0;
        write_value(&mut result, 0, bits);
    }
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

fn restart_state(engine: Engine) {
    let code = [0xd8, 0xc9, 0xde, 0xc2];
    let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    let mut image = stack_image(&code, 0, 0xffc0);
    image.cpu.x87.status.precision = 0;
    write_value(&mut image.cpu, 0, (LEADING + 1, 0x3fff));
    write_value(&mut image.cpu, 1, (0xc000_0000_0000_0000, 0x3fff));
    write_value(&mut image.cpu, 2, (LEADING, 0x7fff));
    let mut first = complete_x87(image.cpu, 2, 0x00c9);
    write_value(&mut first, 0, (0xc000_0000_0000_0002, 0x3fff));
    first.x87.status.precision = 1;
    first.x87.status.c1 = 1;
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: first,
                ram: &[],
                exit: Exit::Interpret
            }]
        )
    );
    let mut second = complete_x87(first, 2, 0x06c2);
    second.x87.status.top = 1;
    second.x87.status.c1 = 0;
    second.x87.tag_word |= 3;
    assert_eq!(
        engine.observe(&linked, &image.input(), 1),
        expected(&image, &[dispatch(second)])
    );
}

#[test]
fn normal_zero_values_and_restart_state() {
    live_arithmetic(Engine::Wasmtime);
    restart_state(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_normal_zero_values_and_restart_state() {
    live_arithmetic(Engine::V8);
    restart_state(Engine::V8);
}

fn admitted_controls(engine: Engine) {
    use crate::support::x87::{set_control, status};
    let code = [0xde, 0xc1];
    let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    for (left, right, gap, opposite) in [
        (LEADING + 1, LEADING + 3, 1, false),
        (LEADING, LEADING, 0, true),
        (LEADING + 1, 0, 0, false),
    ] {
        for (pc, precision) in [(0, 24), (1, 64), (2, 53), (3, 64)] {
            for rc in 0..4 {
                for negative in [false, true] {
                    for masked in [false, true] {
                        let mut image = stack_image(&code, 7, 0x3ffc);
                        image.cpu.x87.status.precision = 0;
                        set_control(
                            &mut image.cpu.x87.control,
                            0x005f | (pc << 8) | (rc << 10) | if masked { PE } else { 0 },
                        );
                        write_value(
                            &mut image.cpu,
                            0,
                            (left, 0x3fff | if negative { 0x8000 } else { 0 }),
                        );
                        write_value(
                            &mut image.cpu,
                            7,
                            (
                                right,
                                if right == 0 {
                                    0
                                } else {
                                    (0x3fff - gap as u16)
                                        | if negative ^ opposite { 0x8000 } else { 0 }
                                },
                            ),
                        );
                        let (bits, flags) = rounding::exact_sum(
                            left,
                            right,
                            gap,
                            negative,
                            negative ^ opposite,
                            precision,
                            rc,
                        );
                        let mut result = complete_x87(image.cpu, 2, 0x06c1);
                        write_value(&mut result, 0, bits);
                        result.x87.tag_word |= 0xc000;
                        result.x87.status = status(
                            0x4500
                                | flags
                                | if !masked && flags & PE != 0 {
                                    PENDING
                                } else {
                                    0
                                },
                        );
                        assert_eq!(
                            engine.observe(&module, &image.input(), 1),
                            expected(&image, &[dispatch(result)]),
                            "admitted sum stays compiled: PC={pc}, RC={rc}, masked={masked}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn admitted_sums_keep_controls_dynamic() {
    admitted_controls(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_admitted_sums_keep_controls_dynamic() {
    admitted_controls(Engine::V8);
}
