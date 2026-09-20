use super::*;

fn lazy_flags(engine: Engine) {
    let mut tables = DescriptorTables::default();
    tables.insert(
        0x27,
        SegmentDescriptor {
            kind: SegmentDescriptorKind::Data {
                writable: false,
                expand_down: false,
            },
            ..descriptor(0, SegmentDefaultSize::Bits32)
        },
    );
    for (extension, result) in [(4, true), (5, false)] {
        let code = [0x0f, 0x00, 0xc0 | (extension << 3)];
        let profile = SegmentProfile::Flat32;
        let mut image = image(&code, profile);
        image.cpu.registers.eax = 0x27;
        image.cpu.flags.status_source.kind = 10;
        image.cpu.flags.status_source.left = 0x7fff_ffff;
        image.cpu.flags.status_source.right = 1;
        let mut cpu = completed(&image, code.len(), result);
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
            &[SegmentQuery::new(&tables, 0x27)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn verification_replaces_zf_while_preserving_other_pending_arithmetic_flags() {
    lazy_flags(Engine::Wasmtime);
}

fn continuing_block(engine: Engine) {
    let profile = SegmentProfile::Segmented16;
    // VERR AX; SETZ DL; VERW AX; SETZ BL.
    let code = [
        0x0f, 0x00, 0xe0, 0x0f, 0x94, 0xc2, 0x0f, 0x00, 0xe8, 0x0f, 0x94, 0xc3,
    ];
    let mut image = image(&code, profile);
    image.cpu.registers.eax = 0x27;
    let mut tables = DescriptorTables::default();
    tables.insert(
        0x27,
        SegmentDescriptor {
            kind: SegmentDescriptorKind::Data {
                writable: false,
                expand_down: false,
            },
            ..descriptor(0, SegmentDefaultSize::Bits32)
        },
    );
    let mut input = image.input();
    input.segment_queries = vec![SegmentQuery::new(&tables, 0x27); 2];
    let first = completed(&image, 3, true);
    let mut second = first;
    second.registers.edx = (second.registers.edx & !0xff) | 1;
    second.eip += 3;
    second.instruction_count += 1;
    let mut third = second;
    third.flags.bytes.zf = 0;
    third.eip += 3;
    third.instruction_count += 1;
    let mut fourth = third;
    fourth.registers.ebx &= !0xff;
    fourth.eip += 3;
    fourth.instruction_count += 1;
    let step = |cpu: CpuState| Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    };
    let mut wanted = expected(
        &image,
        &[step(first), step(second), step(third), step(fourth)],
    );
    wanted
        .events
        .insert(0, Event::QuerySegmentDescriptor { selector: 0x27 });
    wanted
        .events
        .insert(5, Event::QuerySegmentDescriptor { selector: 0x27 });
    assert_eq!(
        engine.observe(TestModule::interpreter_with_profile(profile), &input, 4),
        wanted
    );
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 4, profile);
    let mut wanted = expected(&image, &[step(fourth)]);
    wanted
        .events
        .insert(0, Event::QuerySegmentDescriptor { selector: 0x27 });
    wanted
        .events
        .insert(1, Event::QuerySegmentDescriptor { selector: 0x27 });
    assert_eq!(engine.observe(block, &input, 1), wanted);
}

#[test]
fn verification_continues_in_the_same_block_and_later_instructions_observe_zf() {
    continuing_block(Engine::Wasmtime);
}

fn table_changes(engine: Engine) {
    let profile = SegmentProfile::Flat32;
    let code = [0x0f, 0x00, 0xe8]; // VERW AX.
    let mut image = image(&code, profile);
    image.cpu.registers.eax = 0x27;
    let mut tables = DescriptorTables::default();
    let writable = descriptor(0x8000, SegmentDefaultSize::Bits32);
    tables.insert(0x27, writable);
    image.cpu.segments.fs = tables.resolve_user_segment(Segment::Fs, 0x27).unwrap();
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 1, profile);
    for (replacement, result) in [
        (Some(writable), true),
        (
            Some(SegmentDescriptor {
                kind: SegmentDescriptorKind::Data {
                    writable: false,
                    expand_down: false,
                },
                ..writable
            }),
            false,
        ),
        (None, false),
    ] {
        if let Some(descriptor) = replacement {
            tables.insert(0x27, descriptor);
        } else {
            tables.remove(0x27);
        }
        let mut input = image.input();
        input.segment_queries.push(SegmentQuery::new(&tables, 0x27));
        let cpu = completed(&image, code.len(), result);
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

#[test]
fn the_same_entry_observes_table_edits_while_the_loaded_cache_stays_unchanged() {
    table_changes(Engine::Wasmtime);
}

fn later_fault(engine: Engine) {
    let profile = SegmentProfile::Flat32;
    // VERR AX; MOV ECX,[EBX] (unmapped).
    let code = [0x0f, 0x00, 0xe0, 0x8b, 0x0b];
    let mut image = image(&code, profile);
    image.cpu.registers.eax = 0x27;
    image.cpu.registers.ebx = 0x4000;
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, descriptor(0, SegmentDefaultSize::Bits32));
    let mut input = image.input();
    input.segment_queries.push(SegmentQuery::new(&tables, 0x27));
    let cpu = completed(&image, 3, true);
    let fault = || Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x4000,
            error: 0,
        },
    };
    let mut wanted = expected(
        &image,
        &[
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
            fault(),
        ],
    );
    wanted
        .events
        .insert(0, Event::QuerySegmentDescriptor { selector: 0x27 });
    assert_eq!(engine.observe(TestModule::interpreter(), &input, 2), wanted);
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 2, profile);
    let mut wanted = expected(&image, &[fault()]);
    wanted
        .events
        .insert(0, Event::QuerySegmentDescriptor { selector: 0x27 });
    assert_eq!(engine.observe(block, &input, 1), wanted);
}

#[test]
fn a_later_fault_preserves_completed_verification_and_its_retirement() {
    later_fault(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_verification_flags_continuation_table_changes_and_restart() {
    lazy_flags(Engine::V8);
    continuing_block(Engine::V8);
    table_changes(Engine::V8);
    later_fault(Engine::V8);
}
