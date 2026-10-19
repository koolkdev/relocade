use super::*;

fn comparisons(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // Equality, signed extrema and opposite signs differ in both halves.
    for (equal, greater, left, right, equal_mask, greater_mask) in [
        (
            0x74,
            0x64,
            0x01027e817f0080ff_7e81020100ff7f80,
            0x00037f807fff80fe_7f8102ff0000807f,
            0x00000000ff00ff00_00ffff00ff000000,
            0xff0000ff00ff00ff_000000ff0000ff00,
        ),
        (
            0x75,
            0x65,
            0x000000017ffe8001_0000ffff7fff8000,
            0x0001ffff7fff8000_0000000080007fff,
            0x0000000000000000_ffff000000000000,
            0x0000ffff0000ffff_00000000ffff0000,
        ),
        (
            0x76,
            0x66,
            0x00000000ffffffff_7fffffff80000000,
            0x00000001ffffffff_800000007fffffff,
            0x00000000ffffffff_0000000000000000,
            0x0000000000000000_ffffffff00000000,
        ),
    ] {
        check_binary(&mut checks, equal, left, right, equal_mask);
        check_binary(&mut checks, greater, left, right, greater_mask);
        for (opcode, mask) in [(equal, u128::MAX), (greater, 0)] {
            let code = [0x66, 0x0f, opcode, 0xff]; // xmm7, xmm7
            let image = image(&code);
            let mut cpu = retire(image.cpu, code.len());
            cpu.simd.xmm[7] = mask.to_le_bytes();
            checks.check("self-comparison", &code, &image, &[dispatch(cpu)]);
        }
    }

    // A comparison mask can select whole lanes without changing integer flags.
    let code = [0x66, 0x0f, 0x66, 0xf2, 0x66, 0x0f, 0xdb, 0xf1];
    let mut image = image(&code); // PCMPGTD xmm6,xmm2; PAND xmm6,xmm1
    image.cpu.simd.xmm[6] = 0x00000000ffffffff_7fffffff80000000_u128.to_le_bytes();
    image.cpu.simd.xmm[2] = 0x00000001ffffffff_800000007fffffff_u128.to_le_bytes();
    image.cpu.simd.xmm[1] = 0x123456789abcdef0_13579bdf2468ace0_u128.to_le_bytes();
    let mut compared = retire(image.cpu, 4);
    compared.simd.xmm[6] = 0x0000000000000000_ffffffff00000000_u128.to_le_bytes();
    let mut masked = retire(compared, 4);
    masked.simd.xmm[6] = 0x0000000000000000_13579bdf00000000_u128.to_le_bytes();
    checks.check(
        "comparison mask consumption",
        &code,
        &image,
        &[dispatch(compared), dispatch(masked)],
    );
}

test_frontends!(sse_packed_comparisons, comparisons);
