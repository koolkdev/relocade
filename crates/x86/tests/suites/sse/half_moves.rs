//! Half transfers select the correct lane and preserve every untouched byte.

use super::*;
use crate::support::step::TestModule;
use wasm86_x86::{compile_block_from_bytes, BlockError};

fn registers(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (opcode, destination_bytes, source_bytes) in [(0x12, 0..8, 8..16), (0x16, 8..16, 0..8)] {
        for source in [1, 6] {
            let code = [0x0f, opcode, 0xf0 | source]; // xmm6, xmm1 or xmm6
            let mut image = image(&code);
            image.cpu.simd.xmm[6] = LEFT.to_le_bytes();
            image.cpu.simd.xmm[1] = RIGHT.to_le_bytes();
            let mut cpu = retire(image.cpu, code.len());
            cpu.simd.xmm[6][destination_bytes.clone()]
                .copy_from_slice(&image.cpu.simd.xmm[source as usize][source_bytes.clone()]);
            checks.check(
                "half-register moves select the source half before modifying an aliased destination",
                &code,
                &image,
                &[dispatch(cpu)],
            );
        }
    }
}

fn memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for prefix in [&[][..], &[0x66][..]] {
        for (load_opcode, store_opcode, half) in [(0x12, 0x13, 0..8), (0x16, 0x17, 8..16)] {
            let load = encoding(prefix, load_opcode, 0x30); // xmm6, [eax]
            let store = encoding(prefix, store_opcode, 0x31); // [ecx], xmm6
            let code = [load.as_slice(), store.as_slice()].concat();
            for offset in [1, 0xff8] {
                let mut image = image(&code);
                image.cpu.registers.eax = 0x4000 + offset;
                image.cpu.registers.ecx = 0x6000 + offset;
                image.map(4, 0x8000, false);
                image.map(6, 0xc000, true);
                let bytes = 0x89ab_cdef_0123_4567_u64.to_le_bytes();
                image.data(0x8000 + offset, &bytes);
                image.data(0xc000 + offset - 1, &[0x5a; 10]);
                let mut cpu = retire(image.cpu, load.len());
                cpu.simd.xmm[6][half.clone()].copy_from_slice(&bytes);
                let loaded = cpu;
                cpu = retire(cpu, store.len());
                checks.check(
                    "half loads preserve the other half and unaligned transfers access exactly eight bytes",
                    &code,
                    &image,
                    &[
                        dispatch(loaded),
                        Step {
                            cpu,
                            ram: &[(0xc000 + offset, &bytes)],
                            exit: Exit::Dispatch(cpu.eip),
                        },
                    ],
                );
            }
        }
    }
}

fn split_memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = [0x0f, 0x16, 0x30, 0x66, 0x0f, 0x17, 0x31]; // movhps; movhpd
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4ffd;
    image.cpu.registers.ecx = 0x6ffd;
    image.map(4, 0x8000, false);
    image.map(5, 0xa000, false);
    image.map(6, 0xc000, true);
    image.map(7, 0xe000, true);
    let bytes = 0x89ab_cdef_0123_4567_u64.to_le_bytes();
    image.data(0x8ffd, &bytes[..3]);
    image.data(0xa000, &bytes[3..]);
    let mut cpu = retire(image.cpu, 3);
    cpu.simd.xmm[6][8..].copy_from_slice(&bytes);
    let loaded = cpu;
    cpu = retire(cpu, 4);
    checks.check(
        "high-half transfers traverse noncontiguous pages without shifting the memory address",
        &code,
        &image,
        &[
            dispatch(loaded),
            Step {
                cpu,
                ram: &[(0xcffd, &bytes[..3]), (0xe000, &bytes[3..])],
                exit: Exit::Dispatch(cpu.eip),
            },
        ],
    );
}

fn faults(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for prefix in [&[][..], &[0x66][..]] {
        for opcode in [0x12, 0x13, 0x16, 0x17] {
            let transfer = encoding(prefix, opcode, 0x30);
            let code = [&[0x0f, 0x57, 0xff][..], transfer.as_slice()].concat();
            let mut image = image(&code);
            image.cpu.registers.eax = 0x4ffe;
            image.map(4, 0x8000, true);
            image.data(0x8ffe, &[0x5a; 2]);
            let mut cpu = retire(image.cpu, 3);
            cpu.simd.xmm[7] = [0; 16];
            checks.check(
                "half transfer faults preserve earlier work and do not partially change XMM or memory",
                &code,
                &image,
                &[
                    dispatch(cpu),
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::PageFault {
                            address: 0x5000,
                            error: if opcode & 1 != 0 { 2 } else { 0 },
                        },
                    },
                ],
            );
        }
    }
}

#[test]
fn sse_half_rejected_register_forms() {
    for (prefix, opcodes) in [
        (&[][..], &[0x13, 0x17][..]),
        (&[0x66][..], &[0x12, 0x13, 0x16, 0x17][..]),
    ] {
        for &opcode in opcodes {
            let code = encoding(prefix, opcode, 0xf1);
            assert_eq!(
                compile_block_from_bytes(0x1000, &code, 1).err(),
                Some(BlockError::UnsupportedInstruction {
                    address: 0x1000,
                    opcode: 0x0f
                }),
                "memory-only half transfer rejects {code:02x?}",
            );
            image(&code).check_unchanged_exit(
                TestModule::interpreter(),
                &format!("memory-only half transfer rejects {code:02x?}"),
                Exit::Other(0x0008_000f_0000_1000),
            );
        }
    }
}

test_frontends!(sse_half_registers, registers);
test_frontends!(sse_half_memory, memory);
test_frontends!(sse_half_split_memory, split_memory);
test_frontends!(sse_half_faults, faults);
