use super::*;

fn packing(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (opcode, left, right, result, self_result) in [
        (
            0x63,
            0x0080007fff80ff7f,
            0x7fff0000ffff8000,
            0x7f00ff807f7f8080,
            0x7f7f80807f7f8080,
        ),
        (
            0x67,
            0x010000ff0000ffff,
            0x007f7fffff7f8000,
            0x7fff0000ffff0000,
            0xffff0000ffff0000,
        ),
        (
            0x6b,
            0x00008000ffff7fff,
            0x00007fffffff8000,
            0x7fff80007fff8000,
            0x7fff80007fff8000,
        ),
        (
            0x6b,
            0x7fffffff80000000,
            0x00000000ffffffff,
            0x0000ffff7fff8000,
            0x7fff80007fff8000,
        ),
    ] {
        check_binary(&mut checks, opcode, left, right, result);
        let code = [0x0f, opcode, 0xff];
        let mut image = image(&code);
        write_register_bits(&mut image.cpu, 7, (left, 0x1234));
        let mut cpu = complete(image.cpu, 3);
        write_register_bits(&mut cpu, 7, (self_result, 0xffff));
        checks.check(
            "self-packing uses both copies of the original source",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

test_frontends!(mmx_packing_saturation_and_order, packing);
