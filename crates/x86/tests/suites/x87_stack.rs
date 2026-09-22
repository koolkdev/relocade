//! Exact extended-real movement and logical stack addressing.

#[path = "x87_stack/faults.rs"]
mod faults;
#[path = "x87_stack/memory.rs"]
mod memory;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::status,
};
use wasm86_x86::{CpuState, SegmentProfile, StoredX87Status};

const INDEFINITE: (u64, u16) = (0xc000_0000_0000_0000, 0xffff);

fn initial_image(code: &[u8], top: u8, tags: u16) -> Image {
    let mut image = Image::new(code);
    image.cpu.segments.cs.selector = 0x1b;
    image.cpu.segments.ds.selector = 0x23;
    image.cpu.x87.control_word = 0x037f;
    image.cpu.x87.status = status(0x4720);
    image.cpu.x87.status.top = top;
    image.cpu.x87.tag_word = tags;
    image.cpu.x87.opcode = 0x0654;
    image.cpu.x87.instruction_offset = 0x1234_5678;
    image.cpu.x87.instruction_selector = 0x17;
    image.cpu.x87.data_offset = 0x89ab_cdef;
    image.cpu.x87.data_selector = 0x27;
    for (index, register) in image.cpu.x87.registers.iter_mut().enumerate() {
        register.significand = 0x8000_0000_0000_0000 + index as u64 * 0x1000_0001;
        register.sign_exponent = 0x3fff + index as u16;
        register.reserved.fill(0x80 + index as u8);
    }
    image
}

fn register_bits(cpu: &CpuState, physical: usize) -> (u64, u16) {
    let register = cpu.x87.registers[physical];
    (register.significand, register.sign_exponent)
}

fn write_register_bits(cpu: &mut CpuState, physical: usize, value: (u64, u16)) {
    // Padding belongs to the physical snapshot slot, not to an x87 value.
    cpu.x87.registers[physical].significand = value.0;
    cpu.x87.registers[physical].sign_exponent = value.1;
}

fn complete(mut cpu: CpuState, bytes: u32, opcode: u16) -> CpuState {
    cpu.x87.instruction_offset = cpu.eip;
    cpu.x87.instruction_selector = 0x1b;
    // This emulator enables the P4 opcode-compatibility policy. Register-only
    // operations retain the architecturally undefined data pointer.
    cpu.x87.opcode = opcode;
    cpu.eip = cpu.eip.wrapping_add(bytes);
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

fn dispatch(cpu: CpuState) -> Step<'static> {
    Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    }
}

fn real80(value: (u64, u16)) -> [u8; 10] {
    let mut bytes = [0; 10];
    bytes[..8].copy_from_slice(&value.0.to_le_bytes());
    bytes[8..].copy_from_slice(&value.1.to_le_bytes());
    bytes
}

fn register_moves(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Source indices are resolved before a push or pop changes TOP.
    for (name, code, top, tags, source, destination, final_top, final_tags, opcode) in [
        (
            "FLD ST0 wraps TOP",
            [0xd9, 0xc0],
            0,
            0xc000,
            0,
            7,
            7,
            0,
            0x01c0,
        ),
        (
            "FLD ST2 captures old index",
            [0xd9, 0xc2],
            3,
            0x0030,
            5,
            2,
            2,
            0,
            0x01c2,
        ),
        (
            "FST ST0 aliases source",
            [0xdd, 0xd0],
            4,
            0,
            4,
            4,
            4,
            0,
            0x05d0,
        ),
        (
            "FST ST5 overwrites occupied slot",
            [0xdd, 0xd5],
            6,
            0,
            6,
            3,
            6,
            0,
            0x05d5,
        ),
        (
            "FSTP ST0 writes then empties",
            [0xdd, 0xd8],
            7,
            0,
            7,
            7,
            0,
            0xc000,
            0x05d8,
        ),
        (
            "FSTP ST7 uses pre-pop index",
            [0xdd, 0xdf],
            0,
            0,
            0,
            7,
            1,
            0x0003,
            0x05df,
        ),
    ] {
        let image = initial_image(&code, top, tags);
        let mut cpu = complete(image.cpu, 2, opcode);
        cpu.x87.status.c1 = 0;
        cpu.x87.status.top = final_top;
        cpu.x87.tag_word = final_tags;
        write_register_bits(&mut cpu, destination, register_bits(&image.cpu, source));
        checks.check(name, &code, &image, &[dispatch(cpu)]);
    }

    for (name, code, target) in [
        ("FXCH ST0 self exchange", [0xd9, 0xc8], 6),
        ("FXCH ST5 wraps physical index", [0xd9, 0xcd], 3),
    ] {
        let image = initial_image(&code, 6, 0);
        let mut cpu = complete(image.cpu, 2, 0x0100 | u16::from(code[1]));
        cpu.x87.status.c1 = 0;
        write_register_bits(&mut cpu, 6, register_bits(&image.cpu, target));
        write_register_bits(&mut cpu, target, register_bits(&image.cpu, 6));
        checks.check(name, &code, &image, &[dispatch(cpu)]);
    }
}

fn raw_register_copies(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0xd9, 0xc0, 0xdd, 0xd3, 0xd9, 0xcb];
    for (name, bits, original_tags, pushed_tags, copied_tags) in [
        ("signed zero", (0, 0x8000), 0xfffd, 0x7ffd, 0x7fdd),
        (
            "SNaN",
            (0x8000_0000_0000_1234, 0x7fff),
            0xfffe,
            0xbffe,
            0xbfee,
        ),
        (
            "pseudo-denormal",
            (0x8000_0000_0000_4567, 0),
            0xfffe,
            0xbffe,
            0xbfee,
        ),
        (
            "unsupported",
            (0x0012_3456_789a_bcde, 0x4000),
            0xfffe,
            0xbffe,
            0xbfee,
        ),
    ] {
        let mut image = initial_image(&code, 0, original_tags);
        image.cpu.x87.control_word = 0x007c;
        write_register_bits(&mut image.cpu, 0, bits);
        let mut pushed = complete(image.cpu, 2, 0x01c0);
        pushed.x87.status = status(0x7d20);
        pushed.x87.tag_word = pushed_tags;
        write_register_bits(&mut pushed, 7, bits);
        let mut copied = complete(pushed, 2, 0x05d3);
        copied.x87.tag_word = copied_tags;
        write_register_bits(&mut copied, 2, bits);
        let exchanged = complete(copied, 2, 0x01cb);
        checks.check(
            name,
            &code,
            &image,
            &[dispatch(pushed), dispatch(copied), dispatch(exchanged)],
        );
    }
}

fn stack_controls(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0xdd, 0xc3, 0xd9, 0xf7, 0xd9, 0xf6];
    let image = initial_image(&code, 7, 0);
    let mut freed = complete(image.cpu, 2, 0x05c3);
    freed.x87.tag_word = 0x0030;
    // FFREE leaves all condition codes undefined; this implementation retains them.
    let mut incremented = complete(freed, 2, 0x01f7);
    incremented.x87.status = status(0x4520);
    let mut decremented = complete(incremented, 2, 0x01f6);
    decremented.x87.status = status(0x7d20);
    checks.check(
        "FFREE changes only its tag; TOP rotations do not pop or shuffle payloads",
        &code,
        &image,
        &[
            dispatch(freed),
            dispatch(incremented),
            dispatch(decremented),
        ],
    );

    let code = [0xd9, 0xf7, 0xdf, 0xe0];
    let mut image = initial_image(&code, 7, 0);
    image.cpu.x87.status = StoredX87Status {
        exception_flags: 0xa0,
        top: 0xff,
        c0: 0x81,
        c1: 0x81,
        c2: 0xfe,
        c3: 0x83,
        error_summary: 0xfe,
        busy: 0xff,
    };
    let mut incremented = complete(image.cpu, 2, 0x01f7);
    incremented.x87.status.top = 0;
    incremented.x87.status.c1 = 0;
    let mut observed = incremented;
    observed.eip += 2;
    observed.instruction_count = observed.instruction_count.wrapping_add(1);
    observed.registers.eax = 0x1111_c120;
    checks.check(
        "FINCSTP updates TOP and C1 while preserving other raw status fields",
        &code,
        &image,
        &[dispatch(incremented), dispatch(observed)],
    );
}

test_frontends!(register_movement, register_moves);
test_frontends!(raw_register_movement, raw_register_copies);
test_frontends!(top_and_tags, stack_controls);
