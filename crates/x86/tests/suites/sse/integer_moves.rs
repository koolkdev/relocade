//! MOVD and MOVQ transfer raw integer bits and clear unused XMM destination bits.

use super::*;
use wasm86_x86::{SegmentAttributes, SegmentDefaultSize, SegmentKind};

fn dword_registers(engine: Engine, frontend: Frontend) {
    for (profile, default_size) in [
        (SegmentProfile::Flat32, SegmentDefaultSize::Bits32),
        (SegmentProfile::Segmented16, SegmentDefaultSize::Bits16),
    ] {
        let mut checks = ImageSequences::new(engine, frontend, profile);
        let code = [
            0x66, 0x0f, 0x6e, 0xf1, // xmm6, ecx
            0x66, 0x0f, 0x7e, 0xf7, // edi, xmm6
            0x66, 0x0f, 0x7e, 0xc8, // eax, xmm1
        ];
        let mut image = image(&code);
        image.cpu.segments.cs.attributes =
            SegmentAttributes::new(SegmentKind::Code { readable: true }, default_size);
        image.cpu.registers.ecx = 0x89ab_cdef;
        image.cpu.registers.edi = 0xffff_ffff;
        image.cpu.simd.xmm[1] = RIGHT.to_le_bytes();
        let mut cpu = retire(image.cpu, 4);
        cpu.simd.xmm[6] = 0x89ab_cdef_u128.to_le_bytes();
        let loaded = cpu;
        cpu = retire(cpu, 4);
        cpu.registers.edi = 0x89ab_cdef;
        let stored = cpu;
        cpu = retire(cpu, 4);
        cpu.registers.eax = 0x00ff_ff00;
        checks.check(
            "MOVD uses 32-bit GPRs in either default operand size and clears the upper 96 XMM bits",
            &code,
            &image,
            &[dispatch(loaded), dispatch(stored), dispatch(cpu)],
        );
    }
}

fn quadword_registers(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (prefix, opcode, modrm, destination, source) in [
        (0xf3, 0x7e, 0xf1, 6, 1),
        (0x66, 0xd6, 0xf1, 1, 6),
        (0xf3, 0x7e, 0xff, 7, 7),
        (0x66, 0xd6, 0xff, 7, 7),
    ] {
        let code = [prefix, 0x0f, opcode, modrm];
        let image = image(&code);
        let mut cpu = retire(image.cpu, 4);
        cpu.simd.xmm[destination] = [0; 16];
        cpu.simd.xmm[destination][..8].copy_from_slice(&image.cpu.simd.xmm[source][..8]);
        checks.check(
            "both MOVQ directions clear the upper half, including self-copies",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

fn memory(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (load_prefix, load_opcode, store_opcode, bytes) in
        [(0x66, 0x6e, 0x7e, 4), (0xf3, 0x7e, 0xd6, 8)]
    {
        let code = [
            load_prefix,
            0x0f,
            load_opcode,
            0x30,
            0x66,
            0x0f,
            store_opcode,
            0x31,
        ];
        for offset in [1, 0x1000 - bytes] {
            let mut image = image(&code);
            image.cpu.registers.eax = 0x4000 + offset;
            image.cpu.registers.ecx = 0x6000 + offset;
            image.map(4, 0x8000, false);
            image.map(6, 0xc000, true);
            let value = LEFT.to_le_bytes();
            let value = &value[..bytes as usize];
            image.data(0x8000 + offset, value);
            image.data(0xc000 + offset - 1, &[0x5a; 9]);
            let mut cpu = retire(image.cpu, 4);
            cpu.simd.xmm[6] = [0; 16];
            cpu.simd.xmm[6][..bytes as usize].copy_from_slice(value);
            let loaded = cpu;
            cpu = retire(cpu, 4);
            checks.check(
                "integer transfers accept unaligned addresses and access exactly four or eight bytes",
                &code,
                &image,
                &[
                    dispatch(loaded),
                    Step {
                        cpu,
                        ram: &[(0xc000 + offset, value)],
                        exit: Exit::Dispatch(cpu.eip),
                    },
                ],
            );
        }
    }
}

fn faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (prefix, opcode, store) in [
        (0x66, 0x6e, false),
        (0x66, 0x7e, true),
        (0xf3, 0x7e, false),
        (0x66, 0xd6, true),
    ] {
        let code = [0x66, 0x0f, 0xef, 0xff, prefix, 0x0f, opcode, 0x30]; // pxor xmm7; transfer xmm6
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4ffe;
        image.map(4, 0x8000, true);
        image.data(0x8ffe, &[0x5a; 2]);
        let mut cpu = retire(image.cpu, 4);
        cpu.simd.xmm[7] = [0; 16];
        checks.check(
            "faulted integer transfers preserve earlier work without clearing XMM or partially storing",
            &code,
            &image,
            &[
                dispatch(cpu),
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x5000,
                        error: if store { 2 } else { 0 },
                    },
                },
            ],
        );
    }
}

test_frontends!(sse_movd_registers, dword_registers);
test_frontends!(sse_movq_registers, quadword_registers);
test_frontends!(sse_integer_memory, memory);
test_frontends!(sse_integer_faults, faults);
