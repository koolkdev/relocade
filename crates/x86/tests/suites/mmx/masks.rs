use super::*;

fn mask(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [0x0f, 0xd7, 0xc7]; // PMOVMSKB eax, mm7
    for (bits, mask) in [(0x80ff007f_ff00807f, 0xca), (u64::MAX, 0xff), (0, 0)] {
        let mut image = image(&code);
        image.cpu.registers.eax = u32::MAX;
        write_register_bits(&mut image.cpu, 7, (bits, 0x1234));
        let mut cpu = complete(image.cpu, 3);
        cpu.registers.eax = mask;
        checks.check(
            "byte mask completes MMX without overwriting the source",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

test_frontends!(mmx_byte_mask, mask);
