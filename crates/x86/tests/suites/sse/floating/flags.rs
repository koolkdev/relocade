//! COMI/UCOMI differ only in whether a quiet NaN raises invalid.

use super::*;

fn relations(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for quiet in [false, true] {
            let code = encoding(
                if wide { &[0x66] } else { &[] },
                if quiet { 0x2e } else { 0x2f },
                0xc1,
            );
            for (left, right, flags, invalid, denormal) in [
                (ONE, TWO, 1, false, false),
                (TWO, ONE, 0, false, false),
                (ONE, ONE, 0x40, false, false),
                (NEG_ZERO, ZERO, 0x40, false, false),
                (ZERO, NEG_ZERO, 0x40, false, false),
                (NEG_TWO, NEG_ONE, 1, false, false),
                (NEG_ONE, NEG_TWO, 0, false, false),
                (NEG_INFINITY, ONE, 1, false, false),
                (INFINITY, TWO, 0, false, false),
                (INFINITY, INFINITY, 0x40, false, false),
                (QNAN, ONE, 0x45, !quiet, false),
                (ONE, QNAN, 0x45, !quiet, false),
                (SNAN, QNAN, 0x45, true, false),
                (ONE, SNAN, 0x45, true, false),
                (DENORMAL, ZERO, 0, false, true),
                (NEG_DENORMAL, ZERO, 1, false, true),
                (QNAN, DENORMAL, 0x45, !quiet, false),
                (DENORMAL, SNAN, 0x45, true, false),
            ] {
                for mxcsr in [0x1f80, 0x1f00, 0x1e80, 0x1fc0] {
                    let daz = mxcsr & 0x40 != 0;
                    let raised = u32::from(invalid) | (u32::from(denormal && !daz) << 1);
                    let fault = raised & !(mxcsr >> 7) != 0;
                    let mut image = floating_image(&code, mxcsr);
                    lane(&mut image.cpu.simd.xmm[0], wide, 0, left[wide as usize]);
                    lane(&mut image.cpu.simd.xmm[1], wide, 0, right[wide as usize]);
                    let mut cpu = if fault {
                        image.cpu
                    } else {
                        retire(image.cpu, code.len())
                    };
                    cpu.simd.mxcsr |= raised;
                    if !fault {
                        comparison_flags(&mut cpu, if daz && denormal { 0x40 } else { flags });
                    }
                    let name = format!(
                        "COMI wide={wide} quiet={quiet} left={left:x?} right={right:x?} MXCSR={mxcsr:x}"
                    );
                    checks.check(
                        &name,
                        &code,
                        &image,
                        &[Step {
                            cpu,
                            ram: &[],
                            exit: if fault {
                                Exit::SimdFloatingPoint
                            } else {
                                Exit::Dispatch(cpu.eip)
                            },
                        }],
                    );
                }
            }
        }
    }
}

fn memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        let code = encoding(if wide { &[0x66] } else { &[] }, 0x2f, 0x00);
        let width = if wide { 8 } else { 4 };
        for address in [0x4001, 0x5000 - width, 0x5001 - width] {
            let mut image = floating_image(&code, 0x1f00);
            image.cpu.registers.eax = address;
            image.map(4, 0x8000, false);
            lane(&mut image.cpu.simd.xmm[0], wide, 0, SNAN[wide as usize]);
            let fault = address + width > 0x5000;
            if !fault {
                image.data(
                    address + 0x4000,
                    &ONE[wide as usize].to_le_bytes()[..width as usize],
                );
            }
            let mut cpu = image.cpu;
            if !fault {
                cpu.simd.mxcsr |= 1;
            }
            checks.check(
                "COMI reads exactly one unaligned lane; memory faults precede invalid",
                &code,
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: if fault {
                        Exit::PageFault {
                            address: 0x5000,
                            error: 0,
                        }
                    } else {
                        Exit::SimdFloatingPoint
                    },
                }],
            );
        }
    }
}

test_frontends!(sse_comi_relations, relations);
test_frontends!(sse_comi_memory_priority, memory);
