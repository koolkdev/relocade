use super::*;

fn multiplication(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (opcode, left, right, result) in [
        (
            0xd5,
            0x7fff8000ffff1234,
            0x7fff800000022345,
            0x00010000fffe0404,
        ),
        (
            0xe5,
            0x7fff8000ffff1234,
            0x7fff800000022345,
            0x3fff4000ffff0282,
        ),
        (
            0xd5,
            0x8001ffff00007fff,
            0x7fffffff80000002,
            0xffff00010000fffe,
        ),
        (
            0xe5,
            0x8001ffff00007fff,
            0x7fffffff80000002,
            0xc000000000000000,
        ),
        (
            0xf5,
            0x8000800080008000,
            0x8000800080008000,
            0x8000000080000000,
        ),
        (
            0xf5,
            0x7fff7fff0001ffff,
            0x7fff7fff00010001,
            0x7ffe000200000000,
        ),
        (
            0xf5,
            0x80007fff00030002,
            0x7fff8000fffe0004,
            0x8001000000000002,
        ),
    ] {
        check_binary(&mut checks, opcode, left, right, result);
    }

    let code = [0x0f, 0xf5, 0xff]; // PMADDWD mm7, mm7
    let mut image = image(&code);
    write_register_bits(&mut image.cpu, 7, (0x80008000ffffffff, 0x1234));
    let mut cpu = complete(image.cpu, 3);
    write_register_bits(&mut cpu, 7, (0x8000000000000002, 0xffff));
    checks.check(
        "self-multiplication reads both pairs before writing",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

test_frontends!(mmx_word_products_and_pair_sums, multiplication);

fn unsigned_products(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    check_binary(
        &mut checks,
        0xe4,
        0xffff80000001ffff,
        0xffff8000ffff0001,
        0xfffe400000000000,
    );
    check_binary(
        &mut checks,
        0xf4,
        0xdeadbeefffffffff,
        0x12345678ffffffff,
        0xfffffffe00000001,
    );
    check_binary(
        &mut checks,
        0xf4,
        0xffffffff00000001,
        0xffffffff80000000,
        0x80000000,
    );
}

test_frontends!(mmx_unsigned_products, unsigned_products);
