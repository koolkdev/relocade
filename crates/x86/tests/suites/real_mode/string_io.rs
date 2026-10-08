use super::*;
use crate::support::{
    blocks::BlockModules,
    machine::expected,
    step::{Event, TestModule},
};

fn transfers(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for (code, width, input_port, repeat, backward, address32) in [
        (&[0x6c][..], 1, true, false, false, false),
        (&[0x6d][..], 2, true, false, true, false),
        (&[0x66, 0x6d][..], 4, true, false, false, false),
        (&[0x6e][..], 1, false, false, false, false),
        (&[0x6f][..], 2, false, false, true, false),
        (&[0x66, 0x6f][..], 4, false, false, false, false),
        (&[0xf3, 0x6c][..], 1, true, true, false, false),
        (&[0xf3, 0x6d][..], 2, true, true, true, false),
        (&[0xf3, 0x66, 0x6d][..], 4, true, true, false, false),
        (&[0xf3, 0x6e][..], 1, false, true, false, false),
        (&[0xf3, 0x6f][..], 2, false, true, true, false),
        (&[0xf3, 0x67, 0x66, 0x6f][..], 4, false, true, false, true),
    ] {
        let mut image = image(code);
        image.cpu.registers.edx = 0xabcd_0092;
        image.cpu.registers.ecx = if address32 { 3 } else { 0x1234_0003 };
        let index = if input_port {
            &mut image.cpu.registers.edi
        } else {
            &mut image.cpu.registers.esi
        };
        *index = if address32 { 0x2020 } else { 0xabcd_2020 };
        image.cpu.flags.bytes.df = backward as u8;
        image.map(2, 0x8000, true);
        image.data(0x8000, &[0x5a; 64]);
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, code, 8, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        let mut input = image.input();
        let count = if repeat { 3 } else { 1 };
        let mut events = Vec::new();
        let mut ram = Vec::new();
        for i in 0..count {
            if input_port {
                input.port_reads.push(0x4433_2211 + i);
                let position = if backward {
                    0x8020 - i * width
                } else {
                    0x8020 + i * width
                };
                ram.push((
                    position,
                    (0x4433_2211u32 + i).to_le_bytes()[..width as usize].to_vec(),
                ));
                events.push(Event::PortRead {
                    port: 0x92,
                    bytes: width,
                });
            } else {
                events.push(Event::PortWrite {
                    port: 0x92,
                    bytes: width,
                    value: 0x5a5a_5a5a >> (32 - width * 8),
                });
            }
        }
        let mut cpu = retired(&image, code.len());
        let index = if input_port {
            &mut cpu.registers.edi
        } else {
            &mut cpu.registers.esi
        };
        *index = if backward {
            index.wrapping_sub(count * width)
        } else {
            index.wrapping_add(count * width)
        };
        if repeat {
            cpu.registers.ecx -= count;
        }
        let ram: Vec<_> = ram
            .iter()
            .map(|(at, bytes)| (*at, bytes.as_slice()))
            .collect();
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &ram,
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
        wanted.events.splice(0..0, events);
        assert_eq!(engine.observe(module, &input, 1), wanted, "{code:02x?}");
    }
}
test_frontends!(string_port_transfers, transfers);

fn faults(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for input_port in [false, true] {
        for count in [0, 3] {
            // Address-size 32 lets the first word complete at FFFE; the second
            // element faults at 10000, retaining its count and current index.
            let code = [0xf3, 0x67, if input_port { 0x6d } else { 0x6f }];
            let mut image = image(&code);
            image.cpu.registers.ecx = count;
            image.cpu.registers.esi = 0xfffe;
            image.cpu.registers.edi = 0xfffe;
            image.cpu.registers.edx = 0x1234_0080;
            image.cpu.flags.bytes.df = 0;
            image.map(0xf, 0x8000, true);
            image.data(0x8ffe, &[0x34, 0x12]);
            let module = match frontend {
                Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
                Frontend::Interpreter => {
                    TestModule::interpreter_with_profile(ExecutionProfile::Real16)
                }
            };
            let mut input = image.input();
            let mut cpu = image.cpu;
            let mut ram = Vec::new();
            let mut events = Vec::new();
            let exit = if count == 0 {
                cpu = retired(&image, code.len());
                Exit::Dispatch(cpu.eip)
            } else {
                cpu.registers.ecx = 2;
                if input_port {
                    cpu.registers.edi = 0x10000;
                    input.port_reads = vec![0x5678];
                    events.push(Event::PortRead {
                        port: 0x80,
                        bytes: 2,
                    });
                    ram.push((0x8ffe, &[0x78, 0x56][..]));
                } else {
                    cpu.registers.esi = 0x10000;
                    events.push(Event::PortWrite {
                        port: 0x80,
                        bytes: 2,
                        value: 0x1234,
                    });
                }
                Exit::GeneralProtection { error: 0 }
            };
            let mut wanted = expected(
                &image,
                &[Step {
                    cpu,
                    ram: &ram,
                    exit,
                }],
            );
            wanted.events.splice(0..0, events);
            assert_eq!(engine.observe(module, &input, 1), wanted);
        }
    }
}
test_frontends!(string_port_progress_and_faults, faults);

fn segments_and_mmio(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for input_port in [false, true] {
        let code = [0x64, if input_port { 0x6d } else { 0x6f }];
        let mut image = image(&code);
        image.cpu.segments.ds = cache(Segment::Ds, 0x100);
        image.cpu.segments.es = cache(Segment::Es, 0x200);
        image.cpu.segments.fs = cache(Segment::Fs, 0x300);
        image.cpu.registers.esi = 0xabcd_2000;
        image.cpu.registers.edi = 0x9876_2000;
        image.cpu.registers.edx = 0xffff_0080;
        image.cpu.flags.bytes.df = 0;
        image.map(3, 0x8000, true);
        image.data(0x8000, &[0xff, 0xff]);
        image.data(0xa000, &[0x78, 0x56]);
        let mut input = image.input();
        input.mmio_pages = vec![(4, 0x9000), (5, 0xa000)];
        input.observe_mmio = true;
        let mut cpu = retired(&image, code.len());
        let events = if input_port {
            input.port_reads = vec![0x1234];
            cpu.registers.edi += 2;
            vec![
                Event::PortRead {
                    port: 0x80,
                    bytes: 2,
                },
                Event::MmioWrite {
                    address: 0x4000,
                    value: vec![0x34, 0x12],
                },
            ]
        } else {
            cpu.registers.esi += 2;
            vec![
                Event::MmioRead {
                    address: 0x5000,
                    bytes: 2,
                },
                Event::PortWrite {
                    port: 0x80,
                    bytes: 2,
                    value: 0x5678,
                },
            ]
        };
        let ram = if input_port {
            vec![(0x9000, &[0x34, 0x12][..])]
        } else {
            vec![]
        };
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &ram,
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
        wanted.events.splice(0..0, events);
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, &code, 8, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        assert_eq!(engine.observe(module, &input, 1), wanted);
    }
    // A 16-bit index wraps between complete elements, preserving its high word.
    let code = [0xf3, 0x6d];
    let mut image = image(&code);
    image.cpu.registers.edi = 0xabcd_fffe;
    image.cpu.registers.ecx = 0x1234_0002;
    image.cpu.registers.edx = 0x80;
    image.cpu.flags.bytes.df = 0;
    image.map(0xf, 0x8000, true);
    image.map(0, 0x9000, true);
    let mut input = image.input();
    input.port_reads = vec![0x1122, 0x3344];
    let mut cpu = retired(&image, code.len());
    cpu.registers.edi = 0xabcd_0002;
    cpu.registers.ecx = 0x1234_0000;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[(0x8ffe, &[0x22, 0x11]), (0x9000, &[0x44, 0x33])],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    wanted.events.splice(
        0..0,
        [
            Event::PortRead {
                port: 0x80,
                bytes: 2,
            },
            Event::PortRead {
                port: 0x80,
                bytes: 2,
            },
        ],
    );
    let module = match frontend {
        Frontend::Block => blocks.get(&image.cpu, &code, 8, ExecutionProfile::Real16),
        Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
    };
    assert_eq!(engine.observe(module, &input, 1), wanted);
}
test_frontends!(string_port_segments_and_mmio, segments_and_mmio);

fn privilege(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    let profile = ExecutionProfile::Protected(wasm86_x86::SegmentProfile::Flat32);
    for opcode in [0x6d, 0x6f] {
        for count in [0, 1] {
            let code = [0xf3, opcode];
            let mut image = image(&code);
            image.cpu.segments = Segments::flat32();
            image.cpu.registers.ecx = count;
            let module = match frontend {
                Frontend::Block => blocks.get(&image.cpu, &code, 8, profile),
                Frontend::Interpreter => TestModule::interpreter_with_profile(profile),
            };
            image.check_unchanged_exit(
                engine,
                module,
                "I/O privilege is checked even when REP count is zero",
                Exit::GeneralProtection { error: 0 },
            );
        }
    }
}
test_frontends!(string_port_privilege, privilege);
