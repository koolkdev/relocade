use super::*;

fn rounding(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for rc in 0..4 {
        let controls = 0x1f80 | rc << 13;
        for (opcode, left, right, result, raised) in [
            (0x58, ONE, ONE, TWO, 0),
            (0x5c, TWO, ONE, ONE, 0),
            (0x58, ONE, NEG_ONE, if rc == 1 { NEG_ZERO } else { ZERO }, 0),
            (0x5c, ONE, ONE, if rc == 1 { NEG_ZERO } else { ZERO }, 0),
            (0x58, NEG_ZERO, NEG_ZERO, NEG_ZERO, 0),
            (0x5c, NEG_ZERO, ZERO, NEG_ZERO, 0),
            (
                0x58,
                ONE,
                HALF_ULP,
                if rc == 2 { NEXT_ONE } else { ONE },
                32,
            ),
            (
                0x58,
                NEXT_ONE,
                HALF_ULP,
                if rc == 0 || rc == 2 {
                    [0x3f80_0002, 0x3ff0_0000_0000_0002]
                } else {
                    NEXT_ONE
                },
                32,
            ),
            (0x5c, NEXT_ONE, ONE, [0x3400_0000, 0x3cb0_0000_0000_0000], 0),
            (
                0x58,
                MAX_FINITE,
                MAX_FINITE,
                if rc == 0 || rc == 2 {
                    INFINITY
                } else {
                    MAX_FINITE
                },
                40,
            ),
        ] {
            Case {
                left,
                right,
                result,
                controls,
                raised,
            }
            .check(&mut checks, opcode);
        }
        Case {
            left: ONE,
            right: HALF_ULP,
            result: ONE,
            controls: controls & !0x1000,
            raised: 32,
        }
        .check(&mut checks, 0x58);
        // Exact cancellation is neither tiny nor inexact, even at the largest
        // exponent and with underflow unmasked or flush-to-zero enabled.
        for controls in [controls & !0x800, controls | 0x8000] {
            Case {
                left: MAX_FINITE,
                right: MAX_FINITE,
                result: if rc == 1 { NEG_ZERO } else { ZERO },
                controls,
                raised: 0,
            }
            .check(&mut checks, 0x5c);
        }
    }
}

fn special(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    Case {
        left: TWO,
        right: ONE,
        result: ONE,
        controls: 0x1f00,
        raised: 0,
    }
    .check(&mut checks, 0x5c);
    for opcode in [0x58, 0x5c] {
        for (left, right, result, raised) in [
            (QNAN, SNAN, QNAN, 1),
            (SNAN, QNAN, QUIETED, 1),
            (ONE, SNAN, QUIETED, 1),
            (QNAN, DENORMAL, QNAN, 0),
            (
                INFINITY,
                if opcode == 0x58 {
                    NEG_INFINITY
                } else {
                    INFINITY
                },
                INDEFINITE,
                1,
            ),
        ] {
            for controls in [0x1f80, 0x1f00] {
                Case {
                    left,
                    right,
                    result,
                    raised,
                    controls,
                }
                .check(&mut checks, opcode);
            }
        }
    }
    for (controls, raised, result) in [
        (0x1f80, 2, DENORMAL),
        (0x1e80, 2, DENORMAL),
        (0x1fc0, 0, ZERO),
        (0x9f80, 50, ZERO),
        (0x1780, 18, DENORMAL),
    ] {
        Case {
            left: DENORMAL,
            right: ZERO,
            result,
            controls,
            raised,
        }
        .check(&mut checks, 0x58);
    }
    Case {
        left: MIN_NORMAL,
        right: [0x007f_ffff, 0x000f_ffff_ffff_ffff],
        result: DENORMAL,
        controls: 0x1f80,
        raised: 2,
    }
    .check(&mut checks, 0x5c);
}

fn packed_priority(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = encoding(&[], 0x58, 0xc1);
    for (controls, raised) in [(0x1f80, 33), (0x1f00, 1), (0x0f80, 33)] {
        let mut image = floating_image(&code, controls);
        image.cpu.simd.xmm[0] = [0; 16];
        image.cpu.simd.xmm[1] = [0; 16];
        lane(&mut image.cpu.simd.xmm[0], false, 0, SNAN[0]);
        lane(&mut image.cpu.simd.xmm[0], false, 1, ONE[0]);
        lane(&mut image.cpu.simd.xmm[1], false, 1, HALF_ULP[0]);
        let fault = raised & !(controls >> 7) != 0;
        let mut cpu = if fault {
            image.cpu
        } else {
            retire(image.cpu, code.len())
        };
        cpu.simd.mxcsr |= raised;
        if !fault {
            lane(&mut cpu.simd.xmm[0], false, 0, QUIETED[0]);
        }
        checks.check(
            "packed invalid suppresses sibling precision only when unmasked",
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

fn memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for packed in [false, true] {
            let code = encoding(prefix(wide, packed), 0x58, 0x00);
            let width = if packed {
                16
            } else if wide {
                8
            } else {
                4
            };
            for address in [0x5000 - width, 0x4001, 0x6000] {
                let mut image = floating_image(&code, 0x1f00);
                image.map(4, 0x8000, false);
                image.cpu.registers.eax = address;
                image.cpu.simd.xmm[0] = [0; 16];
                let mut source = [0; 16];
                lane(&mut source, wide, 0, ONE[wide as usize]);
                image.data(address + 0x4000, &source[..width as usize]);
                let fault = if address == 0x6000 {
                    Some(Exit::PageFault { address, error: 0 })
                } else if packed && address % 16 != 0 {
                    Some(Exit::GeneralProtection { error: 0 })
                } else {
                    None
                };
                let mut cpu = image.cpu;
                if fault.is_none() {
                    cpu = retire(cpu, code.len());
                    lane(&mut cpu.simd.xmm[0], wide, 0, ONE[wide as usize]);
                }
                checks.check(
                    "scalar natural access and aligned packed access",
                    &code,
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: fault.unwrap_or(Exit::Dispatch(cpu.eip)),
                    }],
                );
            }
        }
    }
}

test_frontends!(sse_add_sub_rounding, rounding);
test_frontends!(sse_add_sub_special, special);
test_frontends!(sse_add_sub_packed_priority, packed_priority);
test_frontends!(sse_add_sub_memory, memory);
