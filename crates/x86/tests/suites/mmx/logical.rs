use super::*;

const LEFT: u64 = 0xf0ff_00aa_55cc_3381;
const RIGHT: u64 = 0x0f0f_f055_aa33_f080;

fn logical(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (opcode, result, self_result) in [
        (0xdb, 0x000f_0000_0000_3080, LEFT),
        (0xdf, 0x0f00_f055_aa33_c000, 0),
        (0xeb, 0xffff_f0ff_ffff_f381, LEFT),
        (0xef, 0xfff0_f0ff_ffff_c301, 0),
    ] {
        check_binary(&mut checks, opcode, LEFT, RIGHT, result);
        let code = [0x0f, opcode, 0xf6]; // mm6, mm6
        let mut image = image(&code);
        write_register_bits(&mut image.cpu, 6, (LEFT, 0x1234));
        let mut cpu = complete(image.cpu, 3);
        write_register_bits(&mut cpu, 6, (self_result, 0xffff));
        checks.check(
            "same-register logic retains MMX environment effects",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }

    // PXOR followed by a fault must publish the zero and retain its environment.
    let code = [0x0f, 0xef, 0xc0, 0x0f, 0xdb, 0x08];
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4000;
    let mut zeroed = complete(image.cpu, 3);
    write_register_bits(&mut zeroed, 0, (0, 0xffff));
    checks.check(
        "logical source fault publishes earlier definitions",
        &code,
        &image,
        &[
            dispatch(zeroed),
            Step {
                cpu: zeroed,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x4000,
                    error: 0,
                },
            },
        ],
    );
    image.cpu.x87.status.error_summary = 1;
    checks.check(
        "logical instructions deliver pending x87 exceptions",
        &code[..3],
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

test_frontends!(mmx_logical, logical);
