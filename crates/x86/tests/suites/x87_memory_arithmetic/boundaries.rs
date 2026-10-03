//! Memory faults and JIT handoffs precede the current instruction's effects.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::compile_block_from_bytes;

fn fault_ordering(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        Source::Single(0x7f80_0001),
        Source::Double(0x7ff0_0000_0000_0001),
    ] {
        let bytes = source.bytes();
        let address = 0x5001 - bytes.len() as u32;
        let code = source.instruction(0, address);
        let mut image = stack_image(&code, 3, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.map(4, 0x8000, false);
        image.data(address + 0x4000, &bytes[..bytes.len() - 1]);
        checks.check(
            "operand page fault precedes stack and source exceptions",
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
            "pending x87 exception precedes the operand fault",
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

fn source_width(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        Source::Single(0x3f80_0000),
        Source::Double(0x3ff0_0000_0000_0000),
    ] {
        let bytes = source.bytes();
        let address = 0x5000 - bytes.len() as u32;
        let code = [vec![0x66], source.instruction(0, address)].concat();
        let mut image = stack_image(&code, 3, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 3, (0, 0));
        image.map(4, 0x8000, false);
        image.data(address + 0x4000, &bytes);
        let mut result = completed_memory(image.cpu, source, 0);
        result.eip += 1;
        result.x87.data_offset = address;
        write_value(&mut result, 3, (LEADING, 0x3fff));
        checks.check(
            "66 retains the opcode-defined real width",
            &code,
            &image,
            &[dispatch(result)],
        );
    }
}

fn live_values(engine: Engine) {
    let three32 = Source::Single(0x4040_0000);
    let three64 = Source::Double(0x4008_0000_0000_0000);
    let minus_zero = Source::Single(0x8000_0000);
    let code = [
        three32.instruction(0, 0x4000),
        three64.instruction(5, 0x4008),
        minus_zero.instruction(1, 0x4010),
        three64.instruction(4, 0x4008),
    ]
    .concat();
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 4).unwrap());
    let mut image = initial_image(&code, three32, (LEADING, 0xc000));
    image.data(0x8008, &three64.bytes());
    image.data(0x8010, &minus_zero.bytes());
    let mut result = image.cpu;
    for (source, extension, address, value) in [
        (three32, 0, 0x4000, (LEADING, 0x3fff)),
        (three64, 5, 0x4008, (LEADING, 0x4000)),
        (minus_zero, 1, 0x4010, (0, 0x8000)),
        (three64, 4, 0x4008, (0xc000_0000_0000_0000, 0xc000)),
    ] {
        result = completed_memory(result, source, extension);
        result.x87.data_offset = address;
        write_value(&mut result, 3, value);
    }
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(&image, &[dispatch(result)])
    );
}

fn restart_state(engine: Engine) {
    let three = Source::Single(0x4040_0000);
    let denormal = Source::Single(1);
    let code = [
        three.instruction(0, 0x4000),
        denormal.instruction(1, 0x4008),
    ]
    .concat();
    let compiled = compile_block_from_bytes(0x1000, &code, 2).unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    let mut image = initial_image(&code, three, (LEADING, 0xc000));
    image.data(0x8008, &denormal.bytes());
    let mut first = completed_memory(image.cpu, three, 0);
    write_value(&mut first, 3, (LEADING, 0x3fff));
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: first,
                ram: &[],
                exit: Exit::Interpret
            },]
        )
    );
    let mut second = completed_memory(first, denormal, 1);
    second.x87.data_offset = 0x4008;
    second.x87.status.denormal = 1;
    write_value(&mut second, 3, (LEADING, 0x3f6a));
    assert_eq!(
        engine.observe(&linked, &image.input(), 1),
        expected(&image, &[dispatch(second)])
    );
}

fn loaded_denormal_is_an_ordinary_register(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // FLD consumes the narrow source's #D evidence. After FNCLEX, FMUL uses
    // the normal binary80 value and must not raise #D again.
    let code = [0xd9, 0x05, 0, 0x40, 0, 0, 0xdb, 0xe2, 0xd8, 0xc8];
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.status.precision = 0;
    image.map(4, 0x8000, false);
    image.data(0x8000, &1_u32.to_le_bytes());
    let mut loaded = complete_x87(image.cpu, 6, 0x0105);
    loaded.x87.status = status(0x7d02);
    loaded.x87.data_offset = 0x4000;
    loaded.x87.data_selector = 0x23;
    write_value(&mut loaded, 7, (LEADING, 0x3f6a));
    let mut cleared = loaded;
    cleared.eip += 2;
    cleared.instruction_count = cleared.instruction_count.wrapping_add(1);
    cleared.x87.status.denormal = 0;
    let mut multiplied = complete_x87(cleared, 2, 0x00c8);
    write_value(&mut multiplied, 7, (LEADING, 0x3ed5));
    checks.check(
        "loaded denormal evidence is not retained in a register",
        &code,
        &image,
        &[dispatch(loaded), dispatch(cleared), dispatch(multiplied)],
    );
}

test_frontends!(faults, fault_ordering);
test_frontends!(width, source_width);
test_frontends!(load_provenance, loaded_denormal_is_an_ordinary_register);

#[test]
fn memory_values_and_restart_state() {
    live_values(Engine::Wasmtime);
    restart_state(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_memory_values_and_restart_state() {
    live_values(Engine::V8);
    restart_state(Engine::V8);
}
