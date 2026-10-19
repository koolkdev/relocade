use super::*;

fn quotient(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for rc in 0..4 {
        for (left, right, result, raised) in [
            (NEG_TWO, TWO, NEG_ONE, 0),
            (
                ONE,
                [0x4040_0000, 0x4008_0000_0000_0000],
                [
                    if rc == 0 || rc == 2 {
                        0x3eaa_aaab
                    } else {
                        0x3eaa_aaaa
                    },
                    if rc == 2 {
                        0x3fd5_5555_5555_5556
                    } else {
                        0x3fd5_5555_5555_5555
                    },
                ],
                32,
            ),
            (MIN_NORMAL, TWO, [0x0040_0000, 0x0008_0000_0000_0000], 0),
            (DENORMAL, TWO, if rc == 2 { DENORMAL } else { ZERO }, 50),
            (
                MAX_FINITE,
                [0x3f00_0000, 0x3fe0_0000_0000_0000],
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
                raised,
                controls: 0x1f80 | rc << 13,
            }
            .check(&mut checks, 0x5e);
        }
    }
}

fn exceptional(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (left, right, result, raised, controls) in [
        (ONE, ZERO, INFINITY, 4, 0x1f80),
        (NEG_ONE, ZERO, NEG_INFINITY, 4, 0x1f80),
        (ONE, NEG_ZERO, NEG_INFINITY, 4, 0x1f80),
        (ONE, ZERO, INFINITY, 4, 0x1d80),
        (INFINITY, ZERO, INFINITY, 0, 0),
        (ZERO, NEG_ONE, NEG_ZERO, 0, 0),
        (ONE, NEG_INFINITY, NEG_ZERO, 0, 0),
        (ZERO, ZERO, INDEFINITE, 1, 0x1f80),
        (INFINITY, INFINITY, INDEFINITE, 1, 0x1f80),
        (QNAN, ZERO, QNAN, 0, 0x1d80),
        (SNAN, ZERO, QUIETED, 1, 0x1d80),
        (QNAN, SNAN, QNAN, 1, 0x1f00),
        (SNAN, QNAN, QUIETED, 1, 0x1f80),
        (DENORMAL, ZERO, INFINITY, 4, 0x1f80),
        (DENORMAL, ZERO, INDEFINITE, 1, 0x1fc0),
        (MIN_NORMAL, TWO, ZERO, 48, 0x9f80),
        (MIN_NORMAL, TWO, ZERO, 16, 0x1780),
        (ONE, [0x4040_0000, 0x4008_0000_0000_0000], ZERO, 32, 0x0f80),
    ] {
        Case {
            left,
            right,
            result,
            raised,
            controls,
        }
        .check(&mut checks, 0x5e);
    }
}

fn packed_priority(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = encoding(&[], 0x5e, 0xc1);
    for (controls, raised) in [(0x1d80, 4), (0x1f80, 36)] {
        let mut image = floating_image(&code, controls);
        image.cpu.simd.xmm[0] = [0; 16];
        image.cpu.simd.xmm[1] = [0; 16];
        for i in 0..4 {
            lane(&mut image.cpu.simd.xmm[0], false, i, ONE[0]);
            lane(&mut image.cpu.simd.xmm[1], false, i, ONE[0]);
        }
        lane(&mut image.cpu.simd.xmm[1], false, 0, 0);
        lane(&mut image.cpu.simd.xmm[1], false, 1, 0x4040_0000);
        let fault = raised & !(controls >> 7) != 0;
        let mut cpu = if fault {
            image.cpu
        } else {
            retire(image.cpu, code.len())
        };
        cpu.simd.mxcsr |= raised;
        if !fault {
            lane(&mut cpu.simd.xmm[0], false, 0, INFINITY[0]);
            lane(&mut cpu.simd.xmm[0], false, 1, 0x3eaa_aaab);
        }
        checks.check(
            "unmasked zero divide suppresses sibling precision",
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

test_frontends!(sse_divide_quotient, quotient);
test_frontends!(sse_divide_exceptional, exceptional);
test_frontends!(sse_divide_packed_priority, packed_priority);
