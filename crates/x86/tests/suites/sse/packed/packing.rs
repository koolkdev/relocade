use super::*;

fn packing(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // Adjacent saturation boundaries and signed inputs protect both source order and clamps.
    for (opcode, left, right, result) in [
        (
            0x63,
            0x0080007f007e0000_ff81ff80ff7f8000,
            0xffd6002afed4012c_ffff000100ff7fff,
            0xd62a807fff017f7f_7f7f7e0081808080,
        ),
        (
            0x6b,
            0xffff8001ffff8000_ffff7fff80000000,
            0x7fffffff00008000_00007fff00007ffe,
            0x7fff7fff7fff7ffe_8001800080008000,
        ),
        (
            0x67,
            0x7fff010000ff00fe_00010000ffff8000,
            0x8000010000ff0000_002affffff7fff80,
            0x00ffff002a000000_fffffffe01000000,
        ),
    ] {
        check_binary(&mut checks, opcode, left, right, result);
    }
    let code = [0x66, 0x0f, 0x6b, 0xff]; // PACKSSDW xmm7,xmm7
    let mut image = image(&code);
    image.cpu.simd.xmm[7] = 0xffff8001ffff8000_ffff7fff80000000_u128.to_le_bytes();
    let mut cpu = retire(image.cpu, code.len());
    cpu.simd.xmm[7] = 0x8001800080008000_8001800080008000_u128.to_le_bytes();
    checks.check(
        "self-pack concatenates two narrowed copies",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

test_frontends!(sse_packing_saturation_and_order, packing);
