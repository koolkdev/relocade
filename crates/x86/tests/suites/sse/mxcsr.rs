//! MXCSR control policy, literal snapshot preservation and precise memory faults.

use super::*;

fn readback(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let values = [0, 0x1f80, 0xffff]
        .into_iter()
        .chain((0..16).map(|bit| 1 << bit));
    for value in values {
        let code = [0x0f, 0xae, 0x10, 0x0f, 0xae, 0x19]; // load [eax]; store [ecx]
        let mut image = image(&code);
        image.cpu.simd.mxcsr = 0xabcd_9fc5;
        image.cpu.registers.eax = 0x4001;
        image.cpu.registers.ecx = 0x4041;
        image.map(4, 0x8000, true);
        let bytes = u32::to_le_bytes(value);
        image.data(0x8001, &bytes);
        image.data(0x8040, &[0x5a; 6]);
        let mut cpu = retire(image.cpu, 3);
        cpu.simd.mxcsr = value;
        let loaded = cpu;
        cpu = retire(cpu, 3);
        checks.check(
            "MXCSR supports every low bit, including DAZ, without immediate FP exceptions",
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu,
                    ram: &[(0x8041, &bytes)],
                    exit: Exit::Dispatch(cpu.eip),
                },
            ],
        );
    }
}

fn architectural_stores(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (raw, expected) in [
        (CpuState::default().simd.mxcsr, 0x1f80_u32),
        (0xabcd_9fc5, 0x9fc5),
    ] {
        let code = [0x66, 0x0f, 0xae, 0x18]; // operand-size override still stores m32
        let mut image = image(&code);
        image.cpu.simd.mxcsr = raw;
        image.cpu.registers.eax = 0x4ffc;
        image.map(4, 0x8000, true);
        image.data(0x8ff8, &[0x5a; 8]);
        let cpu = retire(image.cpu, code.len());
        checks.check(
            "STMXCSR writes a four-byte architectural word without changing the raw snapshot",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8ffc, &expected.to_le_bytes())],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

fn reserved_bits(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for bit in 16..32 {
        let code = [0x0f, 0xae, 0x10, 0x0f, 0x57, 0xff, 0x0f, 0xae, 0x11];
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4000;
        image.cpu.registers.ecx = 0x4010;
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x5fc3_u32.to_le_bytes());
        image.data(0x8010, &((1_u32 << bit) | 0x1f80).to_le_bytes());
        let mut cpu = retire(image.cpu, 3);
        cpu.simd.mxcsr = 0x5fc3;
        let loaded = cpu;
        cpu = retire(cpu, 3);
        cpu.simd.xmm[7] = [0; 16];
        checks.check(
            "reserved MXCSR loads fault after publishing earlier MXCSR and XMM work",
            &code,
            &image,
            &[
                dispatch(loaded),
                dispatch(cpu),
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                },
            ],
        );
    }
}

fn split_memory(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0x0f, 0xae, 0x10, 0x0f, 0xae, 0x19];
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4ffe;
    image.cpu.registers.ecx = 0x6ffe;
    image.map(4, 0x8000, false);
    image.map(5, 0xc000, false);
    image.map(6, 0xa000, true);
    image.map(7, 0xe000, true);
    image.data(0x8ffe, &[0xc3, 0x5f]);
    image.data(0xc000, &[0, 0]);
    let mut cpu = retire(image.cpu, 3);
    cpu.simd.mxcsr = 0x5fc3;
    let loaded = cpu;
    cpu = retire(cpu, 3);
    checks.check(
        "MXCSR transfers traverse noncontiguous pages",
        &code,
        &image,
        &[
            dispatch(loaded),
            Step {
                cpu,
                ram: &[(0xaffe, &[0xc3, 0x5f]), (0xe000, &[0, 0])],
                exit: Exit::Dispatch(cpu.eip),
            },
        ],
    );
}

fn memory_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (modrm, second_present, error) in [(0x10, false, 0), (0x18, false, 2), (0x18, true, 3)] {
        let code = [0x0f, 0xae, modrm];
        let mut image = image(&code);
        image.cpu.simd.mxcsr = 0xabcd_9fc5;
        image.cpu.registers.eax = 0x4ffe;
        image.map(4, 0x8000, true);
        image.data(0x8ffe, &[0x5a; 2]);
        if second_present {
            image.map(5, 0xc000, false);
        }
        checks.check(
            "failed MXCSR transfers preserve raw state and do not partially store",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error,
                },
            }],
        );
    }
}

test_frontends!(sse_mxcsr_readback, readback);
test_frontends!(sse_mxcsr_architectural_stores, architectural_stores);
test_frontends!(sse_mxcsr_reserved_bits, reserved_bits);
test_frontends!(sse_mxcsr_split_memory, split_memory);
test_frontends!(sse_mxcsr_memory_faults, memory_faults);
