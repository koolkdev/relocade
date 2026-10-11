use super::*;

fn unpack(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (opcode, result, self_result) in [
        (0x60, 0xbb44aa3300229911, 0x4444333322221111),
        (0x61, 0xbbaa443300992211, 0x4433443322112211),
        (0x62, 0xbbaa009944332211, 0x4433221144332211),
        (0x68, 0xff88ee77dd66cc55, 0x8888777766665555),
        (0x69, 0xffee8877ddcc6655, 0x8877887766556655),
        (0x6a, 0xffeeddcc88776655, 0x8877665588776655),
    ] {
        check_binary(
            &mut checks,
            opcode,
            0x8877_6655_4433_2211,
            0xffee_ddcc_bbaa_0099,
            result,
        );
        let code = [0x0f, opcode, 0xff];
        let mut image = image(&code);
        write_register_bits(&mut image.cpu, 7, (0x8877_6655_4433_2211, 0x1234));
        let mut cpu = complete(image.cpu, 3);
        write_register_bits(&mut cpu, 7, (self_result, 0xffff));
        checks.check(
            "self-unpack snapshots both inputs",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

fn unpack_memory_width(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (opcode, result) in [
        (0x60, 0xbb44_aa33_0022_9911),
        (0x61, 0xbbaa_4433_0099_2211),
        (0x62, 0xbbaa_0099_4433_2211),
    ] {
        let code = [0x0f, opcode, 0x38];
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4ffc;
        image.map(4, 0x8000, false);
        image.data(0x8ffc, &0xbbaa_0099_u32.to_le_bytes());
        write_register_bits(&mut image.cpu, 7, (0x8877_6655_4433_2211, 0x1234));
        let mut cpu = complete(image.cpu, 3);
        write_register_bits(&mut cpu, 7, (result, 0xffff));
        checks.check(
            "low unpack reads exactly four bytes",
            &code,
            &image,
            &[dispatch(cpu)],
        );
        let high_code = [0x0f, opcode + 8, 0x38];
        image.data(0x3000, &high_code);
        checks.check(
            "high unpack reads eight bytes and faults before MMX effects",
            &high_code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            }],
        );
        image.data(0x3000, &code);
        image.cpu.registers.eax += 1;
        checks.check(
            "low unpack requires every source byte",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            }],
        );
    }
}

test_frontends!(mmx_unpack_interleaves_selected_half, unpack);
test_frontends!(mmx_unpack_memory_width, unpack_memory_width);
