use super::*;

fn arithmetic(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Carries, borrows, signed extrema and distinct adjacent lanes.
    for (add, sub, left, right, sum, difference) in [
        (
            0xfc,
            0xf8,
            0xff01_fe10_0080_7fff,
            0xffff_03f0_01ff_0101,
            0xfe00_0100_017f_8000,
            0x0002_fb20_ff81_7efe,
        ),
        (
            0xfd,
            0xf9,
            0x0000_8000_7fff_ffff,
            0x0001_ffff_0001_0001,
            0x0001_7fff_8000_0000,
            0xffff_8001_7ffe_fffe,
        ),
        (
            0xfe,
            0xfa,
            0x7fff_ffff_ffff_ffff,
            0x0000_0001_0000_0001,
            0x8000_0000_0000_0000,
            0x7fff_fffe_ffff_fffe,
        ),
    ] {
        check_binary(&mut checks, add, left, right, sum);
        check_binary(&mut checks, sub, left, right, difference);
        let code = [0x0f, sub, 0xff]; // PSUB mm7, mm7
        let image = image(&code);
        let mut cpu = complete(image.cpu, 3);
        write_register_bits(&mut cpu, 7, (0, 0xffff));
        checks.check(
            "same-register subtraction zeros every lane and updates MMX state",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

test_frontends!(mmx_packed_arithmetic, arithmetic);
