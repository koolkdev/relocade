use super::*;

fn registers(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for index in [0, 1, 2, 3, 0xf9, 0xff] {
        let code = [0x0f, 0xc5, 0xc6, index, 0x0f, 0xc4, 0xf1, index];
        let mut image = image(&code);
        image.cpu.registers.eax = u32::MAX;
        image.cpu.registers.ecx = 0xdeadbeef;
        let bits = 0xfedcba98_76543210_u64;
        write_register_bits(&mut image.cpu, 6, (bits, 0x1234));
        let shift = u32::from(index & 3) * 16;
        let mut extracted = complete(image.cpu, 4);
        extracted.registers.eax = ((bits >> shift) & 0xffff) as u32;
        let mut inserted = complete(extracted, 4);
        write_register_bits(
            &mut inserted,
            6,
            (
                (bits & !(0xffff_u64 << shift)) | (0xbeef_u64 << shift),
                0xffff,
            ),
        );
        checks.check(
            "MMX word transfers preserve source exponent until a write",
            &code,
            &image,
            &[dispatch(extracted), dispatch(inserted)],
        );
    }
}

fn memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = [0x0f, 0xc4, 0x30, 0xff];
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4ffe;
    image.map(4, 0x8000, false);
    image.data(0x8ffe, &[0xab, 0xcd]);
    let mut cpu = complete(image.cpu, 4);
    let bits = (image.cpu.x87.registers[6].significand & 0x0000ffffffffffff) | 0xcdab000000000000;
    write_register_bits(&mut cpu, 6, (bits, 0xffff));
    checks.check(
        "MMX PINSRW reads exactly two bytes",
        &code,
        &image,
        &[dispatch(cpu)],
    );
    image.cpu.registers.eax = 0x4fff;
    checks.check(
        "MMX word fault preserves tags and payload",
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
    image.cpu.x87.status.error_summary = 1;
    checks.check(
        "pending x87 exception precedes insertion",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

test_frontends!(mmx_word_registers, registers);
test_frontends!(mmx_word_memory, memory);
