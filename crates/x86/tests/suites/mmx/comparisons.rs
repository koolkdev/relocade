use super::*;

const LEFT: u64 = 0x557f_8001_00ff_7f80;
const RIGHT: u64 = 0x547f_8001_ff00_807f;

fn comparisons(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (eq, gt, left, right, equal, greater) in [
        (
            0x74,
            0x64,
            LEFT,
            RIGHT,
            0x00ff_ffff_0000_0000,
            0xff00_0000_ff00_ff00,
        ),
        (
            0x75,
            0x65,
            0x0000_ffff_7fff_8000,
            0x0001_ffff_8000_7fff,
            0x0000_ffff_0000_0000,
            0x0000_0000_ffff_0000,
        ),
        (
            0x76,
            0x66,
            0x7fff_ffff_8000_0000,
            0x8000_0000_7fff_ffff,
            0,
            0xffff_ffff_0000_0000,
        ),
        (
            0x76,
            0x66,
            0xffff_ffff_8000_0000,
            0xffff_ffff_0000_0000,
            0xffff_ffff_0000_0000,
            0,
        ),
    ] {
        check_binary(&mut checks, eq, left, right, equal);
        check_binary(&mut checks, gt, left, right, greater);
    }
    for (opcode, result) in [
        (0x74, u64::MAX),
        (0x75, u64::MAX),
        (0x76, u64::MAX),
        (0x64, 0),
        (0x65, 0),
        (0x66, 0),
    ] {
        let code = [0x0f, opcode, 0xff];
        let image = image(&code);
        let mut cpu = complete(image.cpu, 3);
        write_register_bits(&mut cpu, 7, (result, 0xffff));
        checks.check(
            "same-register comparisons preserve full-lane mask and environment effects",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
    let code = [0x0f, 0x74, 0xf2, 0x0f, 0xdb, 0xf3]; // PCMPEQB mm6,mm2; PAND mm6,mm3
    let mut image = image(&code);
    write_register_bits(&mut image.cpu, 6, (LEFT, 0x1234));
    write_register_bits(&mut image.cpu, 2, (RIGHT, 0x5678));
    write_register_bits(&mut image.cpu, 3, (0x1234_5678_9abc_def0, 0x2345));
    let mut compared = complete(image.cpu, 3);
    write_register_bits(&mut compared, 6, (0x00ff_ffff_0000_0000, 0xffff));
    let mut masked = complete(compared, 3);
    write_register_bits(&mut masked, 6, (0x0034_5678_0000_0000, 0xffff));
    checks.check(
        "comparison mask feeds a logical operation in the same block",
        &code,
        &image,
        &[dispatch(compared), dispatch(masked)],
    );
}

test_frontends!(mmx_packed_comparisons, comparisons);
