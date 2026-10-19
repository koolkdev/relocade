use super::*;

fn sums(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    check_binary(
        &mut checks,
        0xf6,
        0xffffffffffffffffffffffffffffffff,
        0x0,
        0x7f800000000000007f8,
    );
    check_binary(
        &mut checks,
        0xf6,
        0x0,
        0x10000000000000002,
        0x10000000000000002,
    );
    check_binary(
        &mut checks,
        0xf6,
        0xff0000ff7f80807f0102030405060708,
        0xffff0080807f7f0807060504030201,
        0x3fe0000000000000020,
    );
    let code = [0x66, 0x0f, 0xf6, 0xff];
    let image = image(&code);
    let mut cpu = retire(image.cpu, code.len());
    cpu.simd.xmm[7] = [0; 16];
    checks.check(
        "self differences clear the whole destination",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

test_frontends!(sse_sum_absolute_byte_differences, sums);
