use super::*;

fn shuffle(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let source = 0x7766554433221100_u64;
    let words = [0x1100_u64, 0x3322, 0x5544, 0x7766];
    for control in [0x00, 0x55, 0xff, 0xe4, 0x1b, 0x93] {
        let result = (0..4).fold(0, |bits, lane| {
            bits | (words[usize::from((control >> (2 * lane)) & 3)] << (16 * lane))
        });
        for modrm in [0xf2, 0x30, 0xf6] {
            let code = [0x0f, 0x70, modrm, control];
            let mut image = image(&code);
            write_register_bits(
                &mut image.cpu,
                if modrm == 0xf6 { 6 } else { 2 },
                (source, 0x1234),
            );
            image.cpu.registers.eax = 0x4ff8;
            image.map(4, 0x8000, false);
            image.data(0x8ff8, &source.to_le_bytes());
            let mut cpu = complete(image.cpu, 4);
            write_register_bits(&mut cpu, 6, (result, 0xffff));
            checks.check(
                "MMX shuffle selects source words and publishes MMX state",
                &code,
                &image,
                &[dispatch(cpu)],
            );
        }
    }
    let code = [0x0f, 0x70, 0x30, 0];
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4ff9;
    image.map(4, 0x8000, false);
    checks.check(
        "even a broadcast requires the full m64 source",
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

test_frontends!(mmx_word_shuffle, shuffle);
