use super::*;

fn sums(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    check_binary(&mut checks, 0xf6, 0xffffffffffffffff, 0x0, 0x7f8);
    check_binary(&mut checks, 0xf6, 0x0, 0x2, 0x2);
    check_binary(
        &mut checks,
        0xf6,
        0x102030405060708,
        0x807060504030201,
        0x20,
    );
    let code = [0x0f, 0xf6, 0xff];
    let image = image(&code);
    let mut cpu = complete(image.cpu, code.len() as u32);
    write_register_bits(&mut cpu, 7, (0, 0xffff));
    checks.check(
        "self differences clear the whole destination",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

test_frontends!(mmx_sum_absolute_byte_differences, sums);
