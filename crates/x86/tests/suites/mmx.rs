//! MMX uses physical x87 slots, independent of stack tags and TOP.

#[path = "mmx/arithmetic.rs"]
mod arithmetic;
#[path = "mmx/comparisons.rs"]
mod comparisons;
#[path = "mmx/faults.rs"]
mod faults;
#[path = "mmx/logical.rs"]
mod logical;
#[path = "mmx/state.rs"]
mod state;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{dispatch, stack_image, write_register_bits},
};
use wasm86_x86::{CpuState, SegmentProfile};

fn image(code: &[u8]) -> Image {
    stack_image(code, 5, 0xffff)
}

fn complete(mut cpu: CpuState, bytes: u32) -> CpuState {
    cpu.eip += bytes;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu.x87.tag_word = 0;
    cpu.x87.status.top = 0;
    cpu
}

fn registers(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in 0..8 {
        let destination = (source + 3) & 7;
        for opcode in [0x6f, 0x7f] {
            let modrm = if opcode == 0x6f {
                0xc0 | destination << 3 | source
            } else {
                0xc0 | source << 3 | destination
            };
            let code = [0x0f, opcode, modrm];
            let image = image(&code);
            let mut cpu = complete(image.cpu, 3);
            write_register_bits(
                &mut cpu,
                destination as usize,
                (image.cpu.x87.registers[source as usize].significand, 0xffff),
            );
            checks.check(
                "MOVQ raw physical slots in both directions",
                &code,
                &image,
                &[dispatch(cpu)],
            );
        }
    }
    // A self-copy still sets the exponent, tags and TOP.
    let code = [0x0f, 0x6f, 0xff];
    let image = image(&code);
    let mut cpu = complete(image.cpu, 3);
    cpu.x87.registers[7].sign_exponent = 0xffff;
    checks.check("MOVQ self-copy effects", &code, &image, &[dispatch(cpu)]);

    for (code, loading) in [([0x0f, 0x6e, 0xd3], true), ([0x0f, 0x7e, 0xd3], false)] {
        let mut image = stack_image(&code, 5, 0xffff);
        image.cpu.registers.ebx = 0xdead_beef;
        let mut cpu = complete(image.cpu, 3);
        if loading {
            write_register_bits(&mut cpu, 2, (0xdead_beef, 0xffff));
        } else {
            cpu.registers.ebx = image.cpu.x87.registers[2].significand as u32;
        }
        checks.check(
            "MOVD uses 32-bit GPR and zero extends only on MMX writes",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

fn memory(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (load, store, width, value) in [
        (0x6e, 0x7e, 4, 0x89ab_cdef_u64),
        (0x6f, 0x7f, 8, 0x0123_4567_89ab_cdef),
    ] {
        let code = [0x0f, load, 0x30, 0x0f, store, 0x31, 0x0f, 0x77];
        let mut image = image(&code);
        image.cpu.registers.eax = 0x5000 - width;
        image.cpu.registers.ecx = 0x7000 - width;
        image.map(4, 0x8000, false);
        image.map(6, 0xc000, true);
        let bytes = value.to_le_bytes();
        let bytes = &bytes[..width as usize];
        image.data(0x9000 - width, bytes);
        let mut loaded = complete(image.cpu, 3);
        write_register_bits(&mut loaded, 6, (value, 0xffff));
        let stored = complete(loaded, 3);
        let mut emptied = complete(stored, 2);
        emptied.x87.tag_word = 0xffff;
        checks.check(
            "exact width at page end and EMMS retains payloads",
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: stored,
                    ram: &[(0xd000 - width, bytes)],
                    exit: Exit::Dispatch(stored.eip),
                },
                dispatch(emptied),
            ],
        );
    }
    let code = [0x0f, 0x7f, 0x30]; // MOVQ [eax], mm6, with no preceding MMX write.
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4ffd;
    image.map(4, 0x8000, true);
    image.map(5, 0xc000, true);
    let bytes = image.cpu.x87.registers[6].significand.to_le_bytes();
    let cpu = complete(image.cpu, 3);
    checks.check(
        "unaligned split store retains source exponent",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[(0x8ffd, &bytes[..3]), (0xc000, &bytes[3..])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}

fn segmented(engine: Engine, frontend: Frontend) {
    use wasm86_x86::{SegmentAttributes, SegmentDefaultSize, SegmentKind};
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Segmented16);
    let code = [0x0f, 0x6e, 0x06, 0xfc, 0x4f];
    let mut image = image(&code);
    image.cpu.segments.cs.attributes = SegmentAttributes::new(
        SegmentKind::Code { readable: true },
        SegmentDefaultSize::Bits16,
    );
    image.map(4, 0x8000, false);
    image.data(0x8ffc, &0xdead_beef_u32.to_le_bytes());
    let mut cpu = complete(image.cpu, 5);
    write_register_bits(&mut cpu, 0, (0xdead_beef, 0xffff));
    checks.check(
        "MOVD stays dword in 16-bit code",
        &code,
        &image,
        &[dispatch(cpu)],
    );
    image.cpu.segments.ds.limit = 0x4ffe;
    checks.check(
        "MOVD checks all four bytes against segment limit",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        }],
    );
}

test_frontends!(mmx_register_moves, registers);
test_frontends!(mmx_memory_moves, memory);
test_frontends!(mmx_segmented_moves, segmented);

#[test]
fn mmx_encoding_extents() {
    for code in [
        &[0x0f, 0x77][..],
        &[0x0f, 0x6e, 0xc0],
        &[0x0f, 0x7e, 0xc7],
        &[0x0f, 0x6f, 0x84, 0x8d, 0, 0x40, 0, 0],
        &[0x0f, 0x7f, 0x05, 0, 0x40, 0, 0],
    ] {
        crate::support::encoding::check_length(code);
    }
}

fn check_binary(checks: &mut ImageSequences, opcode: u8, left: u64, right: u64, result: u64) {
    for memory in [false, true] {
        let code = [0x0f, opcode, if memory { 0x30 } else { 0xf2 }]; // mm6, mm2/[eax]
        let mut image = image(&code);
        write_register_bits(&mut image.cpu, 6, (left, 0x1234));
        write_register_bits(&mut image.cpu, 2, (right, 0x5678));
        image.cpu.registers.eax = 0x4ff8;
        image.map(4, 0x8000, false);
        image.data(0x8ff8, &right.to_le_bytes());
        let mut cpu = complete(image.cpu, 3);
        write_register_bits(&mut cpu, 6, (result, 0xffff));
        checks.check(
            "MMX binary operation uses all lanes and preserves unrelated state",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}
