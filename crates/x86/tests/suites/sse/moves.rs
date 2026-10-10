//! Packed move alignment and the distinct upper-lane rules of scalar moves.

use super::*;

fn packed_moves(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (prefix, load_opcode, store_opcode, offset) in [
        (&[][..], 0x28, 0x29, 0),
        (&[0x66][..], 0x28, 0x29, 0),
        (&[0x66][..], 0x6f, 0x7f, 0),
        (&[0xf3][..], 0x6f, 0x7f, 3),
    ] {
        let copy = encoding(prefix, load_opcode, 0xd5); // xmm2, xmm5
        let reverse = encoding(prefix, store_opcode, 0xd3); // xmm3, xmm2
        let load = encoding(prefix, load_opcode, 0x10); // xmm2, [eax]
        let store = encoding(prefix, store_opcode, 0x11); // [ecx], xmm2
        let code = [
            copy.as_slice(),
            reverse.as_slice(),
            load.as_slice(),
            store.as_slice(),
        ]
        .concat();
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4000 + offset;
        image.cpu.registers.ecx = 0x4040 + offset;
        image.map(4, 0x8000, true);
        let bytes = RIGHT.to_le_bytes();
        image.data(0x8000 + offset, &bytes);
        image.data(0x8040 + offset, &[0x5a; 16]);
        let mut cpu = retire(image.cpu, copy.len());
        cpu.simd.xmm[2] = image.cpu.simd.xmm[5];
        let copied = cpu;
        cpu = retire(cpu, reverse.len());
        cpu.simd.xmm[3] = cpu.simd.xmm[2];
        let reversed = cpu;
        cpu = retire(cpu, load.len());
        cpu.simd.xmm[2] = bytes;
        let loaded = cpu;
        cpu = retire(cpu, store.len());
        checks.check(
            "packed register and memory move directions",
            &code,
            &image,
            &[
                dispatch(copied),
                dispatch(reversed),
                dispatch(loaded),
                Step {
                    cpu,
                    ram: &[(0x8040 + offset, &bytes)],
                    exit: Exit::Dispatch(cpu.eip),
                },
            ],
        );
    }
}

fn aligned_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (prefix, opcodes) in [
        (&[][..], [0x28, 0x29]),
        (&[0x66][..], [0x28, 0x29]),
        (&[0x66][..], [0x6f, 0x7f]),
    ] {
        for opcode in opcodes {
            let code = encoding(prefix, opcode, 0x00);
            let mut image = image(&code);
            image.cpu.registers.eax = 0x4001;
            checks.check(
                "misaligned packed moves fault before page translation",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                }],
            );
        }
    }
}

fn scalar_registers(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (prefix, bytes) in [(0xf3, 4), (0xf2, 8)] {
        let code = [
            prefix, 0x0f, 0x10, 0xe1, prefix, 0x0f, 0x11, 0xe7, prefix, 0x0f, 0x10, 0xff,
        ];
        let image = image(&code);
        let mut cpu = retire(image.cpu, 4);
        cpu.simd.xmm[4][..bytes].copy_from_slice(&image.cpu.simd.xmm[1][..bytes]);
        let copied = cpu;
        cpu = retire(cpu, 4);
        cpu.simd.xmm[7][..bytes].copy_from_slice(&image.cpu.simd.xmm[1][..bytes]);
        let reversed = cpu;
        cpu = retire(cpu, 4);
        checks.check(
            "scalar register copies preserve destination upper lanes, including self-copy",
            &code,
            &image,
            &[dispatch(copied), dispatch(reversed), dispatch(cpu)],
        );
    }
}

fn scalar_memory(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (prefix, bytes) in [(0xf3, 4), (0xf2, 8)] {
        // The following pages are unmapped, so an oversized access must fault.
        let code = [prefix, 0x0f, 0x10, 0x30, prefix, 0x0f, 0x11, 0x31];
        let mut image = image(&code);
        image.cpu.registers.eax = 0x5000 - bytes;
        image.cpu.registers.ecx = 0x7000 - bytes;
        image.map(4, 0x8000, false);
        image.map(6, 0xc000, true);
        let value = LEFT.to_le_bytes();
        let value = &value[..bytes as usize];
        image.data(0x9000 - bytes, value);
        image.data(0xcff0, &[0x5a; 16]);
        let mut cpu = retire(image.cpu, 4);
        cpu.simd.xmm[6] = [0; 16];
        cpu.simd.xmm[6][..bytes as usize].copy_from_slice(value);
        let loaded = cpu;
        cpu = retire(cpu, 4);
        checks.check(
            "scalar loads clear upper lanes and transfers use exactly the scalar width",
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu,
                    ram: &[(0xd000 - bytes, value)],
                    exit: Exit::Dispatch(cpu.eip),
                },
            ],
        );
        // Neither scalar width imposes an alignment requirement.
        image.cpu.registers.eax = 0x4001;
        image.cpu.registers.ecx = 0x6001;
        image.data(0x8001, value);
        checks.check(
            "scalar memory moves accept unaligned addresses",
            &code,
            &image,
            &[
                dispatch(CpuState {
                    registers: image.cpu.registers,
                    ..loaded
                }),
                Step {
                    cpu: CpuState {
                        registers: image.cpu.registers,
                        ..cpu
                    },
                    ram: &[(0xc001, value)],
                    exit: Exit::Dispatch(cpu.eip),
                },
            ],
        );
    }
}

fn scalar_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for prefix in [0xf3, 0xf2] {
        for opcode in [0x10, 0x11] {
            let code = [prefix, 0x0f, opcode, 0x00];
            let mut image = image(&code);
            image.cpu.registers.eax = 0x4ffe;
            image.map(4, 0x8000, true);
            image.data(0x8ffe, &[0x5a; 2]);
            checks.check(
                "scalar fault leaves both the XMM destination and memory intact",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x5000,
                        error: if opcode == 0x11 { 2 } else { 0 },
                    },
                }],
            );
        }
    }
}

test_frontends!(sse_packed_moves, packed_moves);
test_frontends!(sse_aligned_move_faults, aligned_faults);
test_frontends!(sse_scalar_register_moves, scalar_registers);
test_frontends!(sse_scalar_memory_moves, scalar_memory);
test_frontends!(sse_scalar_move_faults, scalar_faults);
