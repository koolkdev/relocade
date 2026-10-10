//! Raw XMM transfers, prefix selection and precise memory faults in both decoders.

#[path = "sse/moves.rs"]
mod moves;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
};
use wasm86_x86::{CpuState, SegmentProfile};

const LEFT: u128 = 0x01234567_89abcdef_fedcba98_76543210;
const RIGHT: u128 = 0xf0f00f0f_55aaaa55_3333cccc_00ffff00;

fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu.simd.mxcsr = 0x9fc5;
    image.cpu.simd.xmm =
        std::array::from_fn(|i| (LEFT ^ (i as u128 * RIGHT.wrapping_div(8))).to_le_bytes());
    image
}

fn retire(mut cpu: CpuState, bytes: usize) -> CpuState {
    cpu.eip += bytes as u32;
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

fn encoding(prefix: &[u8], opcode: u8, modrm: u8) -> Vec<u8> {
    [prefix, &[0x0f, opcode, modrm]].concat()
}

fn registers(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for prefix in [&[][..], &[0x66][..]] {
        for source in 0..8 {
            let destination = (source + 3) & 7;
            let copy = encoding(prefix, 0x10, 0xc0 | destination << 3 | source);
            let reverse_destination = (source + 5) & 7;
            let reverse = encoding(prefix, 0x11, 0xc0 | source << 3 | reverse_destination);
            let xor = encoding(prefix, 0x57, 0xc0 | destination << 3 | destination);
            let code = [copy.as_slice(), reverse.as_slice(), xor.as_slice()].concat();
            let image = image(&code);
            let mut cpu = retire(image.cpu, copy.len());
            cpu.simd.xmm[destination as usize] = image.cpu.simd.xmm[source as usize];
            let copied = cpu;
            cpu = retire(cpu, reverse.len());
            cpu.simd.xmm[reverse_destination as usize] = image.cpu.simd.xmm[source as usize];
            let reversed = cpu;
            cpu = retire(cpu, xor.len());
            cpu.simd.xmm[destination as usize] = [0; 16];
            checks.check(
                "XMM encoded registers and both move directions",
                &code,
                &image,
                &[dispatch(copied), dispatch(reversed), dispatch(cpu)],
            );
        }
    }
}

fn transfers(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for prefix in [&[][..], &[0x66][..]] {
        let load = encoding(prefix, 0x10, 0x38); // xmm7, [eax]
        let store = encoding(prefix, 0x11, 0x39); // [ecx], xmm7
        let xor = encoding(prefix, 0x57, 0x3a); // xmm7, [edx]
        let code = [load.as_slice(), store.as_slice(), xor.as_slice()].concat();
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4ff9;
        image.cpu.registers.ecx = 0x5ff9;
        image.cpu.registers.edx = 0x4010;
        image.map(4, 0x8000, true);
        image.map(5, 0xc000, true);
        image.map(6, 0xa000, true);
        let bytes = LEFT.to_le_bytes();
        image.data(0x8ff9, &bytes[..7]);
        image.data(0xc000, &bytes[7..]);
        image.data(0x8010, &RIGHT.to_le_bytes());
        let mut cpu = retire(image.cpu, load.len());
        cpu.simd.xmm[7] = bytes;
        let loaded = cpu;
        cpu = retire(cpu, store.len());
        let stored = cpu;
        cpu = retire(cpu, xor.len());
        cpu.simd.xmm[7] = (LEFT ^ RIGHT).to_le_bytes();
        checks.check(
            "unaligned scattered transfers and aligned XOR",
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: stored,
                    ram: &[(0xcff9, &bytes[..7]), (0xa000, &bytes[7..])],
                    exit: Exit::Dispatch(stored.eip),
                },
                dispatch(cpu),
            ],
        );
    }
}

fn faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for opcode in [0x10, 0x11] {
        for writable in [false, true] {
            let code = [0x0f, 0x57, 0xff, 0x0f, opcode, 0x38];
            let mut image = image(&code);
            image.cpu.registers.eax = 0x4ff9;
            image.map(4, 0x8000, writable);
            image.data(0x8ff9, &[0x5a; 7]);
            let mut cpu = retire(image.cpu, 3);
            cpu.simd.xmm[7] = [0; 16];
            let (address, error) = if opcode == 0x11 && !writable {
                (0x4ff9, 3)
            } else {
                (0x5000, if opcode == 0x11 { 2 } else { 0 })
            };
            checks.check(
                "fault preserves destination and earlier XMM definitions",
                &code,
                &image,
                &[
                    dispatch(cpu),
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::PageFault { address, error },
                    },
                ],
            );
        }
    }
    for prefix in [&[][..], &[0x66][..]] {
        let code = encoding(prefix, 0x57, 0x00);
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4001; // Unmapped and misaligned: #GP precedes #PF.
        checks.check(
            "XOR alignment fault",
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

test_frontends!(sse_register_encodings, registers);
test_frontends!(sse_memory_transfers, transfers);
test_frontends!(sse_precise_faults, faults);

fn segmented_defaults(engine: Engine, frontend: Frontend) {
    use wasm86_x86::{SegmentAttributes, SegmentDefaultSize, SegmentKind};
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Segmented16);
    for prefix in [&[][..], &[0x66][..]] {
        let code = encoding(prefix, 0x10, 0x06); // xmm0, [disp16]
        let code = [code, vec![0x00, 0x40]].concat();
        let mut image = image(&code);
        image.cpu.segments.cs.attributes = SegmentAttributes::new(
            SegmentKind::Code { readable: true },
            SegmentDefaultSize::Bits16,
        );
        image.map(4, 0x8000, false);
        image.data(0x8000, &RIGHT.to_le_bytes());
        let mut cpu = retire(image.cpu, code.len());
        cpu.simd.xmm[0] = RIGHT.to_le_bytes();
        checks.check(
            "mandatory prefix is presence, not effective operand size",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
    // Linear alignment includes the segment base, not only the effective offset.
    let code = [0x0f, 0x57, 0x06, 0x00, 0x40];
    let mut image = image(&code);
    image.cpu.segments.cs.attributes = SegmentAttributes::new(
        SegmentKind::Code { readable: true },
        SegmentDefaultSize::Bits16,
    );
    image.cpu.segments.ds.base = 1;
    checks.check(
        "XOR checks linear alignment",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        }],
    );
    image.cpu.segments.ds.base = 0;
    image.cpu.segments.ds.limit = 0x400e;
    checks.check(
        "segment limit checks the complete vector",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        }],
    );
}

test_frontends!(sse_segmented_defaults, segmented_defaults);
