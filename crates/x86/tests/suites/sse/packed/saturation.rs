use super::*;

fn saturation(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Exact bounds and overflows in both directions differ between signed and unsigned lanes.
    for (opcode, left, right, result) in [
        (
            0xec,
            0x807fff00019c64c0_40ff0100817e807f,
            0x7f80807f00e41cc0_40fffeffff01ff01,
            0xffff807f01807f80_7ffeffff807f807f,
        ),
        (
            0xdc,
            0x807fff00019c64c0_40ff0100817e807f,
            0x7f80807f00e41cc0_40fffeffff01ff01,
            0xffffff7f01ff80ff_80ffffffff7fff80,
        ),
        (
            0xe8,
            0x807fff00019c64c0_40ff0100817e807f,
            0x7f80807f00e41cc0_40fffeffff01ff01,
            0x807f7f8101b84800_00000301827d817e,
        ),
        (
            0xd8,
            0x807fff00019c64c0_40ff0100817e807f,
            0x7f80807f00e41cc0_40fffeffff01ff01,
            0x01007f0001004800_00000000007d007e,
        ),
        (
            0xed,
            0x4000ffff00010000_80017ffe80007fff,
            0x4000fffffffeffff_fffe00020001ffff,
            0x7ffffffeffffffff_80007fff80017ffe,
        ),
        (
            0xdd,
            0x4000ffff00010000_80017ffe80007fff,
            0x4000fffffffeffff_fffe00020001ffff,
            0x8000ffffffffffff_ffff80008001ffff,
        ),
        (
            0xe9,
            0x4000ffff00010000_80017ffe80007fff,
            0x4000fffffffeffff_fffe00020001ffff,
            0x0000000000030001_80037ffc80007fff,
        ),
        (
            0xd9,
            0x4000ffff00010000_80017ffe80007fff,
            0x4000fffffffeffff_fffe00020001ffff,
            0x0000000000000000_00007ffc7fff0000,
        ),
    ] {
        check_binary(&mut checks, opcode, left, right, result);
    }
}

test_frontends!(sse_saturating_arithmetic, saturation);
