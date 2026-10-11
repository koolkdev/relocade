//! Packed XMM operations preserve flags, MXCSR and the independent x87/MMX state.

#[path = "packed/arithmetic.rs"]
mod arithmetic;
#[path = "packed/comparisons.rs"]
mod comparisons;

use super::*;

fn check_binary(checks: &mut ImageSequences, opcode: u8, left: u128, right: u128, result: u128) {
    for memory in [false, true] {
        let code = [0x66, 0x0f, opcode, if memory { 0x30 } else { 0xf2 }]; // xmm6, xmm2/[eax]
        let mut image = image(&code);
        image.cpu.simd.xmm[6] = left.to_le_bytes();
        image.cpu.simd.xmm[2] = right.to_le_bytes();
        image.cpu.x87.status.error_summary = 1;
        image.cpu.x87.status.top = 5;
        image.cpu.x87.tag_word = 0x1234;
        image.cpu.registers.eax = 0x4ff0;
        image.map(4, 0x8000, false);
        image.data(0x8ff0, &right.to_le_bytes());
        let mut cpu = retire(image.cpu, code.len());
        cpu.simd.xmm[6] = result.to_le_bytes();
        checks.check(
            &format!(
                "packed operation {code:02x?} uses all lanes without changing unrelated state"
            ),
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

fn faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // A completed PXOR is published when the following memory operation faults.
    for opcode in [0xd4, 0xdb] {
        // PADDQ and PAND, including a known-zero result.
        let code = [0x66, 0x0f, 0xef, 0xf6, 0x66, 0x0f, opcode, 0x30];
        for (address, exit) in [
            (
                0x5000,
                Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            ),
            (0x5008, Exit::GeneralProtection { error: 0 }),
        ] {
            let mut image = image(&code);
            image.cpu.registers.eax = address;
            image.cpu.x87.status.error_summary = 1;
            let mut cpu = retire(image.cpu, 4);
            cpu.simd.xmm[6] = [0; 16];
            checks.check(
                "aligned binary reads retain faults even when the result is known",
                &code,
                &image,
                &[
                    dispatch(cpu),
                    Step {
                        cpu,
                        ram: &[],
                        exit,
                    },
                ],
            );
        }
    }
}

test_frontends!(sse_packed_binary_faults, faults);
