use super::*;

fn saturation(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (opcode, cases) in [
        (
            0xec,
            [
                (0x40ff0100817e807f, 0x40fffeffff01ff01, 0x7ffeffff807f807f),
                (0x807fff00019c64c0, 0x7f80807f00e41cc0, 0xffff807f01807f80),
            ],
        ),
        (
            0xdc,
            [
                (0x40ff0100817e807f, 0x40fffeffff01ff01, 0x80ffffffff7fff80),
                (0x807fff00019c64c0, 0x7f80807f00e41cc0, 0xffffff7f01ff80ff),
            ],
        ),
        (
            0xe8,
            [
                (0x40ff0100817e807f, 0x40fffeffff01ff01, 0x00000301827d817e),
                (0x807fff00019c64c0, 0x7f80807f00e41cc0, 0x807f7f8101b84800),
            ],
        ),
        (
            0xd8,
            [
                (0x40ff0100817e807f, 0x40fffeffff01ff01, 0x00000000007d007e),
                (0x807fff00019c64c0, 0x7f80807f00e41cc0, 0x01007f0001004800),
            ],
        ),
        (
            0xed,
            [
                (0x80017ffe80007fff, 0xfffe00020001ffff, 0x80007fff80017ffe),
                (0x4000ffff00010000, 0x4000fffffffeffff, 0x7ffffffeffffffff),
            ],
        ),
        (
            0xdd,
            [
                (0x80017ffe80007fff, 0xfffe00020001ffff, 0xffff80008001ffff),
                (0x4000ffff00010000, 0x4000fffffffeffff, 0x8000ffffffffffff),
            ],
        ),
        (
            0xe9,
            [
                (0x80017ffe80007fff, 0xfffe00020001ffff, 0x80037ffc80007fff),
                (0x4000ffff00010000, 0x4000fffffffeffff, 0x0000000000030001),
            ],
        ),
        (
            0xd9,
            [
                (0x80017ffe80007fff, 0xfffe00020001ffff, 0x00007ffc7fff0000),
                (0x4000ffff00010000, 0x4000fffffffeffff, 0x0000000000000000),
            ],
        ),
    ] {
        // Keep every arithmetic boundary vector, with one memory source per opcode.
        for (left, right, result) in cases {
            // mm6, mm2
            check_binary_form(&mut checks, [0x0f, opcode, 0xf2], left, right, result);
        }
        let (left, right, result) = cases[0];
        // mm6, [eax]
        check_binary_form(&mut checks, [0x0f, opcode, 0x30], left, right, result);
    }
}

test_frontends!(mmx_saturating_arithmetic, saturation);
