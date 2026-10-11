use super::*;

fn faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for opcode in [0x6e, 0x6f, 0x7e, 0x7f] {
        let code = [0x0f, opcode, 0x00];
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4ffe;
        image.map(4, 0x8000, true);
        image.data(0x8ffe, &[0x12, 0x34]);
        checks.check(
            "fault has no partial payload, tag, TOP or memory effect",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error: if opcode >= 0x7e { 2 } else { 0 },
                },
            }],
        );
        image.cpu.x87.status.error_summary = 1;
        image.cpu.x87.status.busy = 1;
        checks.check(
            "pending x87 exception precedes MMX memory fault",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::FloatingPoint,
            }],
        );
    }
    for code in [&[0x0f, 0x77][..], &[0x0f, 0x6f, 0xc1]] {
        let mut image = image(code);
        image.cpu.x87.status.error_summary = 1;
        checks.check(
            "EMMS and register MMX wait for pending exceptions",
            code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::FloatingPoint,
            }],
        );
    }
    let code = [0x0f, 0x6f, 0xf9, 0x0f, 0x7f, 0x38];
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4000;
    image.map(4, 0x8000, false);
    let mut cpu = complete(image.cpu, 3);
    write_register_bits(
        &mut cpu,
        7,
        (image.cpu.x87.registers[1].significand, 0xffff),
    );
    checks.check(
        "store fault publishes earlier MMX definitions",
        &code,
        &image,
        &[
            dispatch(cpu),
            Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x4000,
                    error: 3,
                },
            },
        ],
    );
}

test_frontends!(mmx_precise_faults, faults);
