use super::*;

fn addressing(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, ds, page, offset) in [
        (&[0xa1, 0x20, 0][..], 0x1234, 0x12, 0x360),
        (&[0xa1, 0x20, 0][..], 0xffff, 0x100, 0x10),
        (&[0xa1, 0xfe, 0xff][..], 0xffff, 0x10f, 0xfee),
        (&[0x66, 0x67, 0xa1, 0x20, 0, 0, 0][..], 0xffff, 0x100, 0x10),
    ] {
        let mut image = image(code);
        image.cpu.segments.ds = cache(Segment::Ds, ds);
        image.map(page, 0x8000, false);
        image.data(0x8000 + offset, &[0x78, 0x56, 0x34, 0x12]);
        // A20 is enabled: the low alias must not be read.
        image.map(0, 0x9000, false);
        image.data(0x9000 + offset, &[0xee; 4]);
        let mut cpu = retired(&image, code.len());
        cpu.registers.eax = if code[0] == 0x66 {
            0x1234_5678
        } else {
            0x1111_5678
        };
        cases.check(
            "segment shift, offset addition and size overrides",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }

    let code = [0x2e, 0xa3, 0, 0x20]; // MOV CS:[2000],AX
    let mut image = image(&code);
    image.map(2, 0x8000, true);
    let cpu = retired(&image, code.len());
    cases.check(
        "CS overrides allow real-mode data writes",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[(0x8000, &[0x11, 0x11])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );

    let code = [0x8b, 0x46, 0]; // MOV AX,[BP], defaults to SS
    let mut image = super::image(&code);
    image.cpu.registers.ebp = 0xaaaa_0020;
    image.cpu.segments.ss = cache(Segment::Ss, 0x400);
    image.map(4, 0x8000, false);
    image.data(0x8020, &[0xcd, 0xab]);
    let mut cpu = retired(&image, code.len());
    cpu.registers.eax = 0x1111_abcd;
    cases.check(
        "address decoding still selects SS for BP",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}
test_frontends!(segment_addressing, addressing);

fn denied_accesses(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, exit) in [
        (
            &[0xa1, 0xff, 0xff][..],
            Exit::GeneralProtection { error: 0 },
        ),
        (
            &[0x67, 0xa1, 0, 0, 1, 0][..],
            Exit::GeneralProtection { error: 0 },
        ),
        (&[0x36, 0xa1, 0xff, 0xff][..], Exit::StackFault { error: 0 }),
        (
            &[0x67, 0xa1, 0xff, 0xff, 0xff, 0xff][..],
            Exit::GeneralProtection { error: 0 },
        ),
    ] {
        let image = image(code);
        cases.check(
            "segment faults precede physical transfers",
            code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            }],
        );
    }
}
test_frontends!(access_faults, denied_accesses);

fn stack_edges(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for code in [&[0x60][..], &[0x66, 0x60][..]] {
        for sp in [7, 9, 11, 13, 15] {
            let mut image = image(code);
            image.cpu.registers.esp = 0xabcd_0000 | sp;
            cases.check(
                "PUSHA low odd SP faults before stores",
                code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                }],
            );
        }
    }
    for allocation in [0x3000u16, 0x7fff] {
        let code = [0xc8, allocation as u8, (allocation >> 8) as u8, 0];
        let mut image = image(&code);
        image.cpu.registers.esp = 0xabcd_8000;
        image.map(7, 0x8000, true);
        let mut cpu = retired(&image, code.len());
        cpu.registers.esp = 0xabcd_0000 | u32::from(0x7ffeu16.wrapping_sub(allocation));
        cpu.registers.ebp = 0x6666_7ffe;
        cases.check(
            "ENTER allocation needs no backing and may end at FFFF",
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8ffe, &[0x66, 0x66])],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}
test_frontends!(stack_boundaries, stack_edges);

fn physical_policy(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    let code = [0xa3, 0xff, 0x2f];
    let mut rom = image(&code);
    rom.map(2, 0x8000, true);
    rom.map(3, 0x9000, false);
    rom.data(0x8fff, &[0xee]);
    rom.data(0x9000, &[0xee]);
    cases.check(
        "physical routing stores the RAM byte and ignores the ROM write",
        &code,
        &rom,
        &[Step {
            cpu: retired(&rom, code.len()),
            ram: &[(0x8fff, &[0x11])],
            exit: Exit::Dispatch(rom.cpu.eip + code.len() as u32),
        }],
    );
    for code in [&[0xa1, 0, 0x20][..], &[0xa3, 0, 0x20][..]] {
        let image = image(code);
        let mut cpu = retired(&image, code.len());
        if code[0] == 0xa1 {
            cpu.registers.eax = 0x1111_ffff;
        }
        cases.check(
            "physical holes read as FF and ignore writes",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
    let code = [0xf3, 0xad];
    let mut image = image(&code);
    image.cpu.flags.bytes.df = 0;
    image.cpu.segments.ds = cache(Segment::Ds, 0x1000);
    image.cpu.registers.ecx = 0x1234_0002;
    image.cpu.registers.esi = 0xabcd_8ffe;
    image.map(0x18, 0x8000, false);
    image.data(0x8ffe, &[0x78, 0x56]);
    let mut cpu = retired(&image, code.len());
    cpu.registers.eax = 0x1111_ffff;
    cpu.registers.ecx = 0x1234_0000;
    cpu.registers.esi = 0xabcd_9002;
    cases.check(
        "REP continues through a physical hole",
        &code,
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
}
test_frontends!(physical_bus_policy, physical_policy);

fn repeated_copy_remapping(engine: Engine, frontend: Frontend) {
    use crate::support::{
        machine::expected,
        step::{DeviceUpdate, Event, TestModule},
    };
    let code = [0xf3, 0xa4]; // REP MOVSB
    let mut image = image(&code);
    image.cpu.flags.bytes.df = 0;
    image.cpu.registers.ecx = 0x1234_0003;
    image.cpu.registers.esi = 0xabcd_2000;
    image.cpu.registers.edi = 0xdcba_3000;
    image.map(3, 0x9000, true);
    image.data(0x8000, &[0x11, 0x22, 0x33]);
    image.data(0x9000, &[0xee; 3]);
    image.data(0xa000, &[0xee; 3]);
    image.data(0xb000, &[0x66, 0x44, 0x55]);
    let mut input = image.input();
    input.mmio_pages = vec![(2, 0x8000)];
    input.observe_mmio = true;
    input.mmio_updates = vec![DeviceUpdate {
        map: vec![
            (2 * 8, vec![1, 0, 0, 0, 0, 0xb0, 0, 0]),
            (3 * 8, vec![1, 0, 0, 0, 0, 0xa0, 0, 0]),
        ],
        ..DeviceUpdate::default()
    }];
    let block = matches!(frontend, Frontend::Block).then(|| {
        TestModule::new(
            &wasm86_x86::compile_block_from_bytes_with_profile(
                image.cpu.eip,
                &code,
                1,
                ExecutionProfile::Real16,
            )
            .unwrap(),
        )
    });
    let module = block
        .as_ref()
        .unwrap_or_else(|| TestModule::interpreter_with_profile(ExecutionProfile::Real16));
    let mut cpu = retired(&image, code.len());
    cpu.registers.ecx = 0x1234_0000;
    cpu.registers.esi = 0xabcd_2003;
    cpu.registers.edi = 0xdcba_3003;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[(0xa000, &[0x11, 0x44, 0x55])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    wanted.events.insert(
        0,
        Event::MmioRead {
            address: 0x2000,
            bytes: 1,
        },
    );
    assert_eq!(engine.observe(module, &input, 1), wanted);
}
test_frontends!(repeated_copy_uses_live_mappings, repeated_copy_remapping);
