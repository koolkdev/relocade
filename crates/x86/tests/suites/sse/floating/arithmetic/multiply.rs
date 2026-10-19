use super::*;

fn product(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for rc in 0..4 {
        for (left, right, result, raised) in [
            (TWO, NEG_TWO, [0xc080_0000, 0xc010_0000_0000_0000], 0),
            (NEG_ZERO, NEG_ONE, ZERO, 0),
            (ZERO, NEG_ONE, NEG_ZERO, 0),
            (
                NEXT_ONE,
                NEXT_ONE,
                if rc == 2 {
                    [0x3f80_0003, 0x3ff0_0000_0000_0003]
                } else {
                    [0x3f80_0002, 0x3ff0_0000_0000_0002]
                },
                32,
            ),
            (
                MIN_NORMAL,
                [0x3f00_0000, 0x3fe0_0000_0000_0000],
                [0x0040_0000, 0x0008_0000_0000_0000],
                0,
            ),
            (
                MAX_FINITE,
                TWO,
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
            .check(&mut checks, 0x59);
        }
    }
}

fn exceptional(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (left, right, result, raised, controls) in [
        (INFINITY, ZERO, INDEFINITE, 1, 0x1f80),
        (NEG_INFINITY, NEG_ONE, INFINITY, 0, 0x1f80),
        (QNAN, SNAN, QNAN, 1, 0x1f00),
        (SNAN, QNAN, QUIETED, 1, 0x1f80),
        (DENORMAL, QNAN, QNAN, 0, 0),
        (DENORMAL, TWO, [2, 2], 2, 0x1f80),
        (DENORMAL, TWO, ZERO, 0, 0x1fc0),
        (
            MIN_NORMAL,
            [0x3f00_0000, 0x3fe0_0000_0000_0000],
            ZERO,
            48,
            0x9f80,
        ),
        (
            MIN_NORMAL,
            [0x3f00_0000, 0x3fe0_0000_0000_0000],
            ZERO,
            16,
            0x1780,
        ),
        (
            DENORMAL,
            [0x3f00_0000, 0x3fe0_0000_0000_0000],
            ZERO,
            50,
            0x1f80,
        ),
        (
            DENORMAL,
            [0x3f00_0000, 0x3fe0_0000_0000_0000],
            ZERO,
            18,
            0x1780,
        ),
        (NEXT_ONE, NEXT_ONE, ZERO, 32, 0x0f80),
        (MAX_FINITE, TWO, ZERO, 8, 0x1b80),
    ] {
        Case {
            left,
            right,
            result,
            raised,
            controls,
        }
        .check(&mut checks, 0x59);
    }
}

test_frontends!(sse_multiply_product, product);
test_frontends!(sse_multiply_exceptional, exceptional);
