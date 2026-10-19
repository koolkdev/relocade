use super::*;

fn arithmetic(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // Carries and borrows stop at every lane boundary, including bit 64.
    for (add, sub, left, right, sum, difference) in [
        (
            0xfc,
            0xf8,
            0x817e02007fff0100_ff01fe1000807fff,
            0xff02fe010101ff01_ffff03f001ff0101,
            0x8080000180000001_fe000100017f8000,
            0x827c04ff7efe02ff_0002fb20ff817efe,
        ),
        (
            0xfd,
            0xf9,
            0x00017fff8000ffff_000080007fffffff,
            0xffff00010001ffff_0001ffff00010001,
            0x000080008001fffe_00017fff80000000,
            0x00027ffe7fff0000_ffff80017ffefffe,
        ),
        (
            0xfe,
            0xfa,
            0x0000000080000000_7fffffffffffffff,
            0x00000001ffffffff_0000000100000001,
            0x000000017fffffff_8000000000000000,
            0xffffffff80000001_7ffffffefffffffe,
        ),
        (
            0xd4,
            0xfb,
            0x7fffffffffffffff_ffffffffffffffff,
            0x0000000000000001_0000000000000001,
            0x8000000000000000_0000000000000000,
            0x7ffffffffffffffe_fffffffffffffffe,
        ),
    ] {
        check_binary(&mut checks, add, left, right, sum);
        check_binary(&mut checks, sub, left, right, difference);
        let code = [0x66, 0x0f, sub, 0xff]; // PSUB xmm7, xmm7
        let image = image(&code);
        let mut cpu = retire(image.cpu, code.len());
        cpu.simd.xmm[7] = [0; 16];
        checks.check(
            "same-register subtraction zeros every XMM lane",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

test_frontends!(sse_packed_arithmetic, arithmetic);
