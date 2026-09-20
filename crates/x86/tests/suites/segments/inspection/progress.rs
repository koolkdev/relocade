use super::*;

fn lazy_flags(engine: Engine) {
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, descriptor(0, SegmentDefaultSize::Bits32));
    for (opcode, value) in [(2, 0x0040_f300), (3, 0xffff)] {
        for selector in [0x27, 0x33] {
            let code = [0x0f, opcode, 0xc8]; // (E)CX,AX.
            let profile = SegmentProfile::Flat32;
            let mut image = image(&code, profile);
            image.cpu.registers.eax = u32::from(selector);
            image.cpu.flags.status_source.kind = 10;
            image.cpu.flags.status_source.left = 0x7fff_ffff;
            image.cpu.flags.status_source.right = 1;
            let mut cpu = completed(&image, code.len(), selector == 0x27);
            if selector == 0x27 {
                cpu.registers.ecx = value;
            }
            cpu.flags.status_source.kind = 0;
            cpu.flags.bytes.cf = 0;
            cpu.flags.bytes.pf = 1;
            cpu.flags.bytes.af = 1;
            cpu.flags.bytes.sf = 1;
            cpu.flags.bytes.of = 1;
            check_one(
                engine,
                profile,
                &code,
                &image,
                &[SegmentQuery::new(&tables, selector)],
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                },
            );
        }
    }
}

#[test]
fn successful_and_failed_inspection_change_only_zf_among_pending_arithmetic_flags() {
    lazy_flags(Engine::Wasmtime);
}

fn continuation_and_restart(engine: Engine) {
    let profile = SegmentProfile::Flat32;
    // LAR ECX,AX; SETZ DL; VERR AX; SETZ BL; LSL ESI,DI; MOV ECX,[EBP].
    let code = [
        0x0f, 0x02, 0xc8, 0x0f, 0x94, 0xc2, 0x0f, 0x00, 0xe0, 0x0f, 0x94, 0xc3, 0x0f, 0x03, 0xf7,
        0x8b, 0x4d, 0,
    ];
    let mut image = image(&code, profile);
    image.cpu.registers.eax = 0x27;
    image.cpu.registers.edi = 0x33;
    image.cpu.registers.ebp = 0x4000;
    let mut tables = DescriptorTables::default();
    tables.insert(
        0x27,
        SegmentDescriptor {
            kind: SegmentDescriptorKind::Code {
                readable: false,
                conforming: true,
            },
            dpl: PrivilegeLevel::Ring0,
            present: false,
            ..descriptor(0, SegmentDefaultSize::Bits16)
        },
    );
    let mut input = image.input();
    input.segment_queries = [0x27, 0x27, 0x33]
        .map(|selector| SegmentQuery::new(&tables, selector))
        .to_vec();
    let mut states = [image.cpu; 5];
    let mut cpu = image.cpu;
    for (index, state) in states.iter_mut().enumerate() {
        match index {
            0 => {
                cpu.registers.ecx = 0x1d00;
                cpu.flags.bytes.zf = 1;
            }
            1 => cpu.registers.edx = (cpu.registers.edx & !0xff) | 1,
            2 | 4 => cpu.flags.bytes.zf = 0,
            3 => cpu.registers.ebx &= !0xff,
            _ => unreachable!(),
        }
        cpu.eip += 3;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        *state = cpu;
    }
    let mut steps: Vec<_> = states
        .iter()
        .map(|&cpu| Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        })
        .collect();
    let fault = || Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x4000,
            error: 0,
        },
    };
    steps.push(fault());
    let mut wanted = expected(&image, &steps);
    for (index, selector) in [(0, 0x27), (5, 0x27), (10, 0x33)] {
        wanted
            .events
            .insert(index, Event::QuerySegmentDescriptor { selector });
    }
    assert_eq!(engine.observe(TestModule::interpreter(), &input, 6), wanted);

    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 6, profile);
    let mut wanted = expected(&image, &[fault()]);
    for (index, selector) in [0x27, 0x27, 0x33].into_iter().enumerate() {
        wanted
            .events
            .insert(index, Event::QuerySegmentDescriptor { selector });
    }
    assert_eq!(engine.observe(block, &input, 1), wanted);
}

#[test]
fn descriptor_queries_continue_in_one_block_and_a_later_fault_keeps_their_results() {
    continuation_and_restart(Engine::Wasmtime);
}

fn table_changes(engine: Engine) {
    let profile = SegmentProfile::Flat32;
    for opcode in [2, 3] {
        let code = [0x0f, opcode, 0xc8];
        let mut image = image(&code, profile);
        image.cpu.registers.eax = 0x27;
        let mut tables = DescriptorTables::default();
        let original = descriptor(0x8000, SegmentDefaultSize::Bits32);
        tables.insert(0x27, original);
        image.cpu.segments.fs = tables.resolve_user_segment(Segment::Fs, 0x27).unwrap();
        let mut blocks = BlockModules::default();
        let block = blocks.get(&image.cpu, &code, 1, profile);
        for (replacement, rights, limit) in [
            (Some(original), 0x0040_f300, 0xffff),
            (
                Some(SegmentDescriptor {
                    limit: SegmentLimit::pages(0x12345).unwrap(),
                    default_size: SegmentDefaultSize::Bits16,
                    available: true,
                    present: false,
                    ..original
                }),
                0x0090_7300,
                0x1234_5fff,
            ),
            (None, 0, 0),
        ] {
            if let Some(descriptor) = replacement {
                tables.insert(0x27, descriptor);
            } else {
                tables.remove(0x27);
            }
            let mut input = image.input();
            input.segment_queries.push(SegmentQuery::new(&tables, 0x27));
            let mut cpu = completed(&image, code.len(), replacement.is_some());
            if replacement.is_some() {
                cpu.registers.ecx = if opcode == 2 { rights } else { limit };
            }
            let mut wanted = expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
            wanted
                .events
                .insert(0, Event::QuerySegmentDescriptor { selector: 0x27 });
            for module in [block, TestModule::interpreter()] {
                assert_eq!(engine.observe(module, &input, 1), wanted);
            }
        }
    }
}

#[test]
fn current_table_rights_and_limits_are_observed_without_changing_loaded_caches() {
    table_changes(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_inspection_flags_continuation_table_changes_and_restart() {
    lazy_flags(Engine::V8);
    continuation_and_restart(Engine::V8);
    table_changes(Engine::V8);
}
