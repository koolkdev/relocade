use wasm86_x86::{compile_block_from_bytes, StatusFlags};

use crate::support::{
    machine::{both, byte_register_image, expected, Exit, Image, Step},
    step::TestModule,
};

#[path = "exchange_arithmetic/cmpxchg.rs"]
mod cmpxchg;
#[path = "exchange_arithmetic/decoding.rs"]
mod decoding;
#[path = "exchange_arithmetic/memory.rs"]
mod memory;
#[path = "exchange_arithmetic/xadd.rs"]
mod xadd;

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags.kind = 0;
    image.cpu.flags.status = StatusFlags {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 1,
        of: 1,
    };
    image.cpu.flags.non_status = [0, 1, 0, 0, 0, 0xa5];
    image
}

const EXCHANGES_AND_FLAGS: &[u8] = &[
    0x0f, 0xc0, 0xe0, // XADD AL,AH
    0x0f, 0x92, 0xc2, // SETC DL
    0x0f, 0xb0, 0xe3, // CMPXCHG BL,AH, succeeds
    0x0f, 0x94, 0xc6, // SETZ DH
    0x66, 0x0f, 0xc1, 0x17, // XADD [EDI],DX
    0x66, 0x0f, 0xb1, 0xd6, // CMPXCHG SI,DX, fails comparison
    0x0f, 0xb1, 0x4d, 0, // CMPXCHG [EBP],ECX, read-only and unequal
];
const SUM_WRITE: &[(u32, &[u8])] = &[(0x8000, &[0, 1])];

fn exchanges_and_flags() -> (Image, Vec<Step<'static>>) {
    struct Completed {
        next: u32,
        eax: u32,
        ebx: u32,
        edx: u32,
        kind: u8,
        left: u32,
        right: u32,
        ram: &'static [(u32, &'static [u8])],
    }
    let mut image = image(EXCHANGES_AND_FLAGS);
    image.cpu.registers.eax = 0x4433_01ff;
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.ebp = 0x5000;
    image.cpu.registers.esi = 0x7777_7ffe;
    image.cpu.registers.edi = 0x4000;
    image.cpu.instruction_count = 0xffff_fffd;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, false);
    image.data(0x7fff, &[0x5a, 0xff, 0xff, 0x5a]);
    image.data(0x9fff, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a]);
    let mut cpu = image.cpu;
    let mut steps = Vec::new();
    for completed in [
        Completed {
            next: 0x1003,
            eax: 0x4433_ff00,
            ebx: 0x4000,
            edx: 0xccbb_aa99,
            kind: 2,
            left: 0xff,
            right: 1,
            ram: &[],
        },
        Completed {
            next: 0x1006,
            eax: 0x4433_ff00,
            ebx: 0x4000,
            edx: 0xccbb_aa01,
            kind: 2,
            left: 0xff,
            right: 1,
            ram: &[],
        },
        Completed {
            next: 0x1009,
            eax: 0x4433_ff00,
            ebx: 0x40ff,
            edx: 0xccbb_aa01,
            kind: 1,
            left: 0,
            right: 0,
            ram: &[],
        },
        Completed {
            next: 0x100c,
            eax: 0x4433_ff00,
            ebx: 0x40ff,
            edx: 0xccbb_0101,
            kind: 1,
            left: 0,
            right: 0,
            ram: &[],
        },
        Completed {
            next: 0x1010,
            eax: 0x4433_ff00,
            ebx: 0x40ff,
            edx: 0xccbb_ffff,
            kind: 6,
            left: 0xffff,
            right: 0x0101,
            ram: SUM_WRITE,
        },
        Completed {
            next: 0x1014,
            eax: 0x4433_7ffe,
            ebx: 0x40ff,
            edx: 0xccbb_ffff,
            kind: 5,
            left: 0xff00,
            right: 0x7ffe,
            ram: &[],
        },
    ] {
        cpu.registers.eax = completed.eax;
        cpu.registers.ebx = completed.ebx;
        cpu.registers.edx = completed.edx;
        cpu.flags.kind = completed.kind;
        cpu.flags.left = completed.left;
        cpu.flags.right = completed.right;
        cpu.eip = completed.next;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        steps.push(Step {
            cpu,
            ram: completed.ram,
            exit: Exit::Dispatch(cpu.eip),
        });
    }
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x5000,
            error: 3,
        },
    });
    (image, steps)
}

#[test]
fn exchange_results_and_consumed_flags_survive_a_later_write_fault() {
    let (image, steps) = exchanges_and_flags();
    both(
        TestModule::interpreter(),
        "completed exchanges publish before a failed CMPXCHG write access",
        EXCHANGES_AND_FLAGS,
        7,
        &image,
        &steps,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn exchange_arithmetic_and_fault_publication_execute_in_optimizing_v8() {
    let (image, steps) = exchanges_and_flags();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), steps.len()),
        expected(&image, &steps),
        "interpreter",
    );
    let block =
        TestModule::new(&compile_block_from_bytes(image.cpu.eip, EXCHANGES_AND_FLAGS, 7).unwrap());
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: steps.last().unwrap().cpu,
                ram: SUM_WRITE,
                exit: steps.last().unwrap().exit,
            }]
        ),
        "snapshot block",
    );
}
