use super::*;

fn integer(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let source: [u8; 16] = std::array::from_fn(|i| 0x80 + i as u8);
    for (prefix, width, first) in [(0x66, 4, 0), (0xf2, 2, 0), (0xf3, 2, 4)] {
        for control in [0x00, 0x55, 0xff, 0xe4, 0x1b, 0x93] {
            let mut expected = source;
            for lane in 0..4 {
                let from = (first + usize::from((control >> (2 * lane)) & 3)) * width;
                let to = (first + lane) * width;
                expected[to..to + width].copy_from_slice(&source[from..from + width]);
            }
            for modrm in [0xf2, 0x30, 0xf6] {
                // xmm6,xmm2/[eax]/xmm6
                let code = [prefix, 0x0f, 0x70, modrm, control];
                let mut image = image(&code);
                image.cpu.simd.xmm[2] = source;
                if modrm == 0xf6 {
                    image.cpu.simd.xmm[6] = source;
                }
                image.cpu.x87.status.error_summary = 1;
                image.cpu.registers.eax = 0x4ff0;
                image.map(4, 0x8000, false);
                image.data(0x8ff0, &source);
                let mut cpu = retire(image.cpu, code.len());
                cpu.simd.xmm[6] = expected;
                checks.check(
                    "shuffle copies all source bits, including the untouched half",
                    &code,
                    &image,
                    &[dispatch(cpu)],
                );
            }
        }
    }
}

fn faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for prefix in [0x66, 0xf2, 0xf3] {
        let code = [prefix, 0x0f, 0x70, 0x30, 0];
        for (address, exit) in [
            (
                0x5000,
                Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            ),
            (0x5001, Exit::GeneralProtection { error: 0 }),
        ] {
            let mut image = image(&code);
            image.cpu.registers.eax = address;
            checks.check(
                "shuffle requires aligned m128 before changing its destination",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit,
                }],
            );
        }
    }
}

test_frontends!(sse_integer_shuffle, integer);
test_frontends!(sse_integer_shuffle_faults, faults);
