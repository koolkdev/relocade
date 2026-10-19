use super::*;

fn multiplication(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // Signed high products include negative results in both halves. Pair sums wrap at 32 bits.
    for (opcode, left, right, result) in [
        (
            0xd5,
            0x8001ffff00007fff_7fff8000ffff1234,
            0x7fffffff80000002_7fff800000022345,
            0xffff00010000fffe_00010000fffe0404,
        ),
        (
            0xe5,
            0x8001ffff00007fff_7fff8000ffff1234,
            0x7fffffff80000002_7fff800000022345,
            0xc000000000000000_3fff4000ffff0282,
        ),
        (
            0xf5,
            0x7fff7fff80007fff_0001ffff80008000,
            0x7fff7fff7fff8000_0001000180008000,
            0x7ffe000280010000_0000000080000000,
        ),
        (
            0xf5,
            0x8000800080008000_8000800080008000,
            0x8000800080008000_8000800080008000,
            0x8000000080000000_8000000080000000,
        ),
    ] {
        check_binary(&mut checks, opcode, left, right, result);
    }
    let code = [0x66, 0x0f, 0xf5, 0xff]; // PMADDWD xmm7,xmm7
    let mut image = image(&code);
    image.cpu.simd.xmm[7] = 0x80008000ffffffff_000100017fff7fff_u128.to_le_bytes();
    let mut cpu = retire(image.cpu, code.len());
    cpu.simd.xmm[7] = 0x8000000000000002_000000027ffe0002_u128.to_le_bytes();
    checks.check(
        "self-multiplication snapshots all pairs",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

test_frontends!(sse_word_products_and_pair_sums, multiplication);

fn unsigned_products(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    check_binary(
        &mut checks,
        0xe4,
        0xffff80007fff0000ffff80000001ffff,
        0xffffffffffffffffffff8000ffff0001,
        0xfffe7fff7ffe0000fffe400000000000,
    );
    check_binary(
        &mut checks,
        0xf4,
        0x12345678ffffffffdeadbeefffffffff,
        0xffffffff8000000012345678ffffffff,
        0x7fffffff80000000fffffffe00000001,
    );
    check_binary(
        &mut checks,
        0xf4,
        0xffffffff00000000ffffffff00000001,
        0xffffffffffffffff80000000,
        0x80000000,
    );
}

test_frontends!(sse_unsigned_products, unsigned_products);
