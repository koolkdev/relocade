//! Byte masks select writes while complete operands retain precise fault checks.

#[path = "masked_stores/addressing.rs"]
mod addressing;
#[path = "masked_stores/mmio.rs"]
mod mmio;

use crate::support::{
    encoding::check_unsupported_at_page_end,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{stack_image, write_register_bits},
};
use wasm86_x86::{CpuState, SegmentProfile};

const DATA: [u8; 16] = [
    0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe, 0x0f,
];
const MASK: [u8; 16] = [
    0, 0x80, 0x7f, 0xff, 1, 0x81, 0x40, 0xfe, 0x80, 0, 0x7f, 0xff, 0xfe, 1, 0x81, 0x80,
];
const MIXED: [u8; 16] = [
    0x5a, 0x21, 0x5a, 0x43, 0x5a, 0x65, 0x5a, 0x87, 0x98, 0x5a, 0x5a, 0xcb, 0xdc, 0x5a, 0xfe, 0x0f,
];

fn code(bytes: usize) -> Vec<u8> {
    let mut code = vec![];
    if bytes == 16 {
        code.push(0x66);
    }
    code.extend([0x0f, 0xf7, 0xf2]); // source6, mask2, implicit DS:(E)DI
    code
}

fn image(code: &[u8], mask: [u8; 16]) -> Image {
    let mut image = stack_image(code, 5, 0xaaaa);
    image.cpu.simd.mxcsr = 0x9fc5;
    image.cpu.simd.xmm[6] = DATA;
    image.cpu.simd.xmm[2] = mask;
    write_register_bits(
        &mut image.cpu,
        6,
        (u64::from_le_bytes(DATA[..8].try_into().unwrap()), 0x1234),
    );
    write_register_bits(
        &mut image.cpu,
        2,
        (u64::from_le_bytes(mask[..8].try_into().unwrap()), 0x5678),
    );
    image
}

fn retired(mut cpu: CpuState, length: usize, bytes: usize) -> CpuState {
    cpu.eip += length as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    if bytes == 8 {
        cpu.x87.tag_word = 0;
        cpu.x87.status.top = 0;
    }
    cpu
}

fn payloads(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for bytes in [8, 16] {
        let code = code(bytes);
        for (mask, result) in [
            ([0; 16], [0x5a; 16]),
            ([0x7f; 16], [0x5a; 16]),
            ([0x80; 16], DATA),
            (MASK, MIXED),
        ] {
            let mut image = image(&code, mask);
            image.cpu.registers.edi = 0x4ffd;
            image.map(4, 0x8000, true);
            image.map(5, 0xc000, true);
            image.data(0x8ffd, &[0x5a; 3]);
            image.data(0xc000, &[0x5a; 16]);
            let cpu = retired(image.cpu, code.len(), bytes);
            checks.check(
                "only mask sign bits select bytes across scattered pages",
                &code,
                &image,
                &[Step {
                    cpu,
                    ram: &[(0x8ffd, &result[..3]), (0xc000, &result[3..bytes])],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }

        let mut alias = code.clone();
        *alias.last_mut().unwrap() = 0xf6;
        let mut image = image(&alias, MASK);
        image.cpu.registers.edi = 0x4011;
        image.map(4, 0x8000, true);
        image.data(0x8011, &[0x5a; 16]);
        let result = [
            0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed,
            0xfe, 0x5a,
        ];
        let cpu = retired(image.cpu, alias.len(), bytes);
        checks.check(
            "source and mask may name the same register",
            &alias,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8011, &result[..bytes])],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

fn faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for bytes in [8, 16] {
        let code = code(bytes);
        for mask in [
            [0; 16],
            [0x80; 16],
            [0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ] {
            for writable in [false, true] {
                let mut image = image(&code, mask);
                image.cpu.registers.edi = 0x4ffd;
                image.map(4, 0x8000, writable);
                image.data(0x8ffd, &[0x5a; 3]);
                checks.check(
                    "whole-span policy faults before any selected byte is stored",
                    &code,
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: Exit::PageFault {
                            address: if writable { 0x5000 } else { 0x4ffd },
                            error: if writable { 2 } else { 3 },
                        },
                    }],
                );
            }
        }
        let mut image = image(&code, [0; 16]);
        image.cpu.registers.edi = 0x4010;
        image.cpu.x87.status.error_summary = 1;
        image.map(4, 0x8000, true);
        let (cpu, exit) = if bytes == 8 {
            (image.cpu, Exit::FloatingPoint)
        } else {
            let cpu = retired(image.cpu, code.len(), bytes);
            (cpu, Exit::Dispatch(cpu.eip))
        };
        checks.check(
            "only MMX admits pending x87 exceptions, including an empty mask",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit,
            }],
        );
    }
}

test_frontends!(masked_payloads, payloads);
test_frontends!(masked_faults, faults);

#[test]
fn masked_stores_require_two_registers() {
    for bytes in [8, 16] {
        let mut code = code(bytes);
        *code.last_mut().unwrap() = 0x34; // memory/SIB is rejected before fetching a SIB
        check_unsupported_at_page_end(&code, 0x0f);
    }
}
