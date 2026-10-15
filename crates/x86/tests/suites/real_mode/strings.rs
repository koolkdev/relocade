use super::*;
use crate::support::{blocks::BlockModules, machine::expected, step::TestModule};

fn direct_transfers(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for (code, width, backward) in [
        (&[0xf3, 0xa4][..], 1, false),
        (&[0xf3, 0xa5][..], 2, true),
        (&[0xf3, 0x66, 0xa5][..], 4, false),
    ] {
        let mut image = image(code);
        let contents: Vec<_> = (1..=5 * width as u8).collect();
        let offset = if backward { 4 * width } else { 0 };
        image.cpu.flags.bytes.df = u8::from(backward);
        image.cpu.registers.ecx = 0xcccc_0005;
        image.cpu.registers.esi = 0xaaaa_2100 + offset;
        image.cpu.registers.edi = 0xbbbb_5100 + offset;
        image.map(2, 0x8000, false);
        image.map(5, 0x9000, true);
        image.data(0x8100, &contents);
        let mut cpu = retired(&image, code.len());
        cpu.registers.ecx = 0xcccc_0000;
        let advance = if backward {
            0u32.wrapping_sub(5 * width)
        } else {
            5 * width
        };
        cpu.registers.esi = cpu.registers.esi.wrapping_add(advance);
        cpu.registers.edi = cpu.registers.edi.wrapping_add(advance);
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, code, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[(0x9100, &contents)],
                    exit: Exit::Dispatch(cpu.eip),
                }]
            ),
            "ROM-to-RAM copy must complete without handoff: {code:02x?}"
        );
    }
    for (code, value, bytes) in [
        (&[0xf3, 0xaa][..], 0x1122_337b, vec![0x7b; 3]),
        (
            &[0xf3, 0x66, 0xab][..],
            0x4433_2211,
            [0x11, 0x22, 0x33, 0x44].repeat(3),
        ),
    ] {
        let mut image = image(code);
        image.cpu.flags.bytes.df = 0;
        image.cpu.registers.ecx = 0xcccc_0003;
        image.cpu.registers.edi = 0xbbbb_5100;
        image.cpu.registers.eax = value;
        image.map(5, 0x9000, true);
        let mut cpu = retired(&image, code.len());
        cpu.registers.ecx = 0xcccc_0000;
        cpu.registers.edi += bytes.len() as u32;
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, code, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[(0x9100, &bytes)],
                    exit: Exit::Dispatch(cpu.eip),
                }]
            ),
            "fill must complete without handoff: {code:02x?}"
        );
    }
}
test_frontends!(physical_bulk_completion, direct_transfers);

fn aliases_and_empty_ranges(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa4];
    let mut blocks = BlockModules::default();
    for count in [0, 4] {
        let mut image = image(&code);
        image.cpu.flags.bytes.df = 0;
        image.cpu.registers.ecx = 0xcccc_0000 | count;
        image.cpu.registers.esi = if count == 0 { 0xaaaa_ffff } else { 0xaaaa_2100 };
        image.cpu.registers.edi = if count == 0 { 0xbbbb_ffff } else { 0xbbbb_5101 };
        if count != 0 {
            image.map(2, 0x8000, false);
            image.map(5, 0x8000, true);
            image.data(0x8100, &[1, 2, 3, 4, 5]);
        }
        let mut cpu = retired(&image, code.len());
        cpu.registers.ecx = 0xcccc_0000;
        cpu.registers.esi += count;
        cpu.registers.edi += count;
        let ram: Vec<_> = if count == 0 {
            vec![]
        } else {
            vec![(0x8101, &[1, 1, 1, 1][..])]
        };
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &ram,
                    exit: Exit::Dispatch(cpu.eip),
                }]
            ),
            "physical aliases retain scalar MOVS ordering, and zero count touches no memory"
        );
    }
}
test_frontends!(
    physical_bulk_aliases_and_zero_count,
    aliases_and_empty_ranges
);

fn crossing_range(engine: Engine, frontend: Frontend) {
    let code = [0xf3, 0xa4];
    let mut image = image(&code);
    image.cpu.flags.bytes.df = 0;
    image.cpu.registers.ecx = 2;
    image.cpu.registers.esi = 0x2fff;
    image.cpu.registers.edi = 0x5000;
    image.map(2, 0x8000, false);
    image.map(3, 0xa000, false);
    image.map(5, 0x9000, true);
    image.data(0x8fff, &[0x12]);
    image.data(0xa000, &[0x34]);
    let interpreter = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    let block = TestModule::new(
        &wasm86_x86::compile_block_from_bytes_with_profile(
            image.cpu.eip,
            &code,
            1,
            ExecutionProfile::Real16,
        )
        .unwrap(),
    );
    if matches!(frontend, Frontend::Block) {
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Interpret,
                }]
            )
        );
    }
    let block = block.with_interpreter(interpreter);
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => interpreter,
    };
    let mut cpu = retired(&image, code.len());
    cpu.registers.ecx = 0;
    cpu.registers.esi = 0x3001;
    cpu.registers.edi = 0x5002;
    let wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[(0x9000, &[0x12, 0x34])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    assert_eq!(engine.observe(module, &image.input(), 1), wanted);
}
test_frontends!(
    physical_bulk_crossing_restarts_without_effects,
    crossing_range
);
