use super::*;

fn rounding(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for rc in 0..4 {
        let controls = 0x1f80 | rc << 13;
        let root_two = [
            if rc == 2 { 0x3fb5_04f4 } else { 0x3fb5_04f3 },
            if rc == 0 || rc == 2 {
                0x3ff6_a09e_667f_3bcd
            } else {
                0x3ff6_a09e_667f_3bcc
            },
        ];
        for (right, result, raised) in [
            ([0x4080_0000, 0x4010_0000_0000_0000], TWO, 0),
            (ONE, ONE, 0),
            (TWO, root_two, 32),
            (MIN_NORMAL, [0x2000_0000, 0x2000_0000_0000_0000], 0),
        ] {
            Case {
                left: SNAN,
                right,
                result,
                controls,
                raised,
            }
            .check(&mut checks, 0x51);
        }
        Case {
            left: SNAN,
            right: TWO,
            result: root_two,
            controls: controls & !0x1000,
            raised: 32,
        }
        .check(&mut checks, 0x51);
    }
}

fn special(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for controls in [0x1f80, 0x1f00] {
        for (right, result, raised) in [
            (ZERO, ZERO, 0),
            (NEG_ZERO, NEG_ZERO, 0),
            (INFINITY, INFINITY, 0),
            (NEG_ONE, INDEFINITE, 1),
            (NEG_INFINITY, INDEFINITE, 1),
            (QNAN, QNAN, 0),
            (SNAN, QUIETED, 1),
        ] {
            // Destination is ignored, including its low signaling NaN.
            Case {
                left: SNAN,
                right,
                result,
                controls,
                raised,
            }
            .check(&mut checks, 0x51);
        }
    }
    for (controls, right, result, raised) in [
        (0x1fc0, DENORMAL, ZERO, 0),
        (0x1fc0, [0x8000_0001, 0x8000_0000_0000_0001], NEG_ZERO, 0),
        (0x1f80, [0x8000_0001, 0x8000_0000_0000_0001], INDEFINITE, 1),
        (
            0x1f80,
            [0x0020_0000, 0x0004_0000_0000_0000],
            [0x1f80_0000, 0x1ff0_0000_0000_0000],
            2,
        ),
        (
            0x9f80,
            [0x0020_0000, 0x0004_0000_0000_0000],
            [0x1f80_0000, 0x1ff0_0000_0000_0000],
            2,
        ),
        (
            0x1e80,
            [0x0020_0000, 0x0004_0000_0000_0000],
            [0x1f80_0000, 0x1ff0_0000_0000_0000],
            2,
        ),
    ] {
        Case {
            left: SNAN,
            right,
            result,
            controls,
            raised,
        }
        .check(&mut checks, 0x51);
    }
}

test_frontends!(sse_square_root_rounding, rounding);
test_frontends!(sse_square_root_special, special);
