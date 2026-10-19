//! The eight legacy predicates and exception aggregation across packed lanes.

use super::*;

fn predicates(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // Bits enumerate the predicates that are true, independently of NaN signaling.
    for (left, right, truths) in [
        (ONE, TWO, 0b1001_0110),
        (TWO, ONE, 0b1111_0000),
        (NEG_ZERO, ZERO, 0b1010_0101),
        (QNAN, ONE, 0b0111_1000),
        (ONE, SNAN, 0b0111_1000),
    ] {
        for wide in [false, true] {
            for packed in [false, true] {
                for predicate in 0..8 {
                    let mut code = encoding(prefix(wide, packed), 0xc2, 0xc1);
                    code.push(predicate);
                    let mut image = floating_image(&code, 0x1f80);
                    let count = if packed {
                        if wide {
                            2
                        } else {
                            4
                        }
                    } else {
                        1
                    };
                    for index in 0..count {
                        lane(&mut image.cpu.simd.xmm[0], wide, index, left[wide as usize]);
                        lane(
                            &mut image.cpu.simd.xmm[1],
                            wide,
                            index,
                            right[wide as usize],
                        );
                    }
                    let mut cpu = retire(image.cpu, code.len());
                    let invalid =
                        right == SNAN || (left == QNAN && [1, 2, 5, 6].contains(&predicate));
                    cpu.simd.mxcsr |= u32::from(invalid);
                    for index in 0..count {
                        lane(
                            &mut cpu.simd.xmm[0],
                            wide,
                            index,
                            if truths & (1 << predicate) != 0 {
                                u64::MAX
                            } else {
                                0
                            },
                        );
                    }
                    checks.check(
                        "legacy CMP predicate truth table",
                        &code,
                        &image,
                        &[dispatch(cpu)],
                    );
                }
            }
        }
    }
}

fn exceptions(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for packed in [false, true] {
            let mut code = encoding(prefix(wide, packed), 0xc2, 0xc1);
            code.push(0); // EQ: quiet NaNs do not raise invalid.
            for (mxcsr, nan, raised, fault) in [
                (0x1f80, QNAN, if packed { 2 } else { 0 }, false),
                (0x1f80, SNAN, if packed { 3 } else { 1 }, false),
                (0x1f00, SNAN, if packed { 3 } else { 1 }, true),
                (0x1e80, QNAN, if packed { 2 } else { 0 }, packed),
                (0x1e80, SNAN, if packed { 3 } else { 1 }, packed),
                (0x1e00, SNAN, if packed { 3 } else { 1 }, true),
                (0x1fc0, SNAN, 1, false),
            ] {
                let mut image = floating_image(&code, mxcsr);
                let count = if wide { 2 } else { 4 };
                for index in 0..count {
                    lane(
                        &mut image.cpu.simd.xmm[0],
                        wide,
                        index,
                        if index == 0 {
                            nan[wide as usize]
                        } else {
                            ZERO[wide as usize]
                        },
                    );
                    lane(
                        &mut image.cpu.simd.xmm[1],
                        wide,
                        index,
                        DENORMAL[wide as usize],
                    );
                }
                let mut cpu = if fault {
                    image.cpu
                } else {
                    retire(image.cpu, code.len())
                };
                cpu.simd.mxcsr |= raised;
                if !fault {
                    for index in 0..if packed { count } else { 1 } {
                        let bits = if index != 0 && mxcsr & 0x40 != 0 {
                            u64::MAX
                        } else {
                            0
                        };
                        lane(&mut cpu.simd.xmm[0], wide, index, bits);
                    }
                }
                checks.check(
                    "CMP collects all lane conditions before faulting or replacing XMM",
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

fn memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for packed in [false, true] {
            let mut code = encoding(prefix(wide, packed), 0xc2, 0x00);
            code.push(3); // UNORD
            let width = if packed {
                16
            } else if wide {
                8
            } else {
                4
            };
            for (address, exit) in [
                (0x5000 - width, None),
                (
                    0x4001,
                    if packed {
                        Some(Exit::GeneralProtection { error: 0 })
                    } else {
                        None
                    },
                ),
                (
                    0x6000,
                    Some(Exit::PageFault {
                        address: 0x6000,
                        error: 0,
                    }),
                ),
            ] {
                let mut image = floating_image(&code, 0x1f00);
                image.cpu.registers.eax = address;
                image.map(4, 0x8000, false);
                lane(&mut image.cpu.simd.xmm[0], wide, 0, SNAN[wide as usize]);
                let mut cpu = image.cpu;
                if exit.is_none() {
                    cpu.simd.mxcsr |= 1;
                }
                checks.check(
                    "CMP memory width and alignment precede numerical conditions",
                    &code,
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: exit.unwrap_or(Exit::SimdFloatingPoint),
                    }],
                );
            }
        }
    }
}

fn mixed_lanes(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = [0x0f, 0xc2, 0xc1, 2]; // CMPPS LE: less, greater, equal, unordered.
    let mut image = floating_image(&code, 0x1f80);
    for (index, (left, right)) in [(ONE, TWO), (TWO, ONE), (NEG_ZERO, ZERO), (QNAN, ONE)]
        .into_iter()
        .enumerate()
    {
        lane(&mut image.cpu.simd.xmm[0], false, index, left[0]);
        lane(&mut image.cpu.simd.xmm[1], false, index, right[0]);
    }
    let mut cpu = retire(image.cpu, code.len());
    cpu.simd.mxcsr = 0x1f81;
    cpu.simd.xmm[0] = [
        255, 255, 255, 255, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0,
    ];
    checks.check(
        "packed comparison orders independent lanes",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

test_frontends!(sse_cmp_predicates, predicates);
test_frontends!(sse_cmp_exceptions, exceptions);
test_frontends!(sse_cmp_memory, memory);
test_frontends!(sse_cmp_mixed_lanes, mixed_lanes);
