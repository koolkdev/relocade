use super::*;

fn unpack(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Distinct source bytes expose the chosen half, lane width and interleave order.
    for (opcode, result) in [
        (0x60, 0x8707860685058404_8303820281018000),
        (0x68, 0x8f0f8e0e8d0d8c0c_8b0b8a0a89098808),
        (0x61, 0x8786070685840504_8382030281800100),
        (0x69, 0x8f8e0f0e8d8c0d0c_8b8a0b0a89880908),
        (0x62, 0x8786858407060504_8382818003020100),
        (0x6a, 0x8f8e8d8c0f0e0d0c_8b8a89880b0a0908),
        (0x6c, 0x8786858483828180_0706050403020100),
        (0x6d, 0x8f8e8d8c8b8a8988_0f0e0d0c0b0a0908),
    ] {
        check_binary(
            &mut checks,
            opcode,
            0x0f0e0d0c0b0a0908_0706050403020100,
            0x8f8e8d8c8b8a8988_8786858483828180,
            result,
        );
    }
    for (opcode, result) in [
        (0x6c, 0x0706050403020100_0706050403020100_u128),
        (0x6d, 0x0f0e0d0c0b0a0908_0f0e0d0c0b0a0908_u128),
    ] {
        let code = [0x66, 0x0f, opcode, 0xff]; // PUNPCKL/HQDQ xmm7,xmm7
        let mut image = image(&code);
        image.cpu.simd.xmm[7] = 0x0f0e0d0c0b0a0908_0706050403020100_u128.to_le_bytes();
        let mut cpu = retire(image.cpu, code.len());
        cpu.simd.xmm[7] = result.to_le_bytes();
        checks.check(
            "self-unpack snapshots the selected qword",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

fn unpack_span(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Segmented16);
    for opcode in [0x60, 0x68] {
        check_m128_span(&mut checks, &[0x66, 0x0f, opcode, 0x36, 0x00, 0x40]);
    }
}

test_frontends!(sse_unpack_lane_order, unpack);
test_frontends!(sse_unpack_memory_span, unpack_span);
