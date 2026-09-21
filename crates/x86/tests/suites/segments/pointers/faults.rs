use super::*;

fn source_faults(engine: Engine) {
    for stack_source in [false, true] {
        for word in [false, true] {
            for (limit, first_present, exit) in [
                (
                    0xfff,
                    true,
                    if stack_source {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::GeneralProtection { error: 0 }
                    },
                ),
                (
                    0xfff,
                    false,
                    if stack_source {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::GeneralProtection { error: 0 }
                    },
                ),
                (
                    0xffff,
                    true,
                    Exit::PageFault {
                        address: 0x5000,
                        error: 0,
                    },
                ),
                (
                    0xffff,
                    false,
                    Exit::PageFault {
                        address: if word { 0x4ffe } else { 0x4ffc },
                        error: 0,
                    },
                ),
            ] {
                let mut code = vec![];
                if word {
                    code.push(0x66);
                }
                code.extend([0x0f, 0xb2]); // LSS EAX,[EBX] / [EBP].
                code.extend(if stack_source {
                    &[0x45, 0][..]
                } else {
                    &[0x03]
                });
                let mut image = Image::new(&code);
                image.cpu.registers.ebx = if word { 0xffe } else { 0xffc };
                image.cpu.registers.ebp = image.cpu.registers.ebx;
                image.cpu.segments[if stack_source {
                    Segment::Ss
                } else {
                    Segment::Ds
                }] = data(0x4000, limit);
                if first_present {
                    image.map(4, 0x8000, false);
                }
                image.data(
                    if word { 0x8ffe } else { 0x8ffc },
                    if word {
                        &[0x78, 0x56]
                    } else {
                        &[0x78, 0x56, 0x34, 0x92]
                    },
                );
                // Both destinations and the resolver remain untouched on any source fault.
                check_one(
                    engine,
                    SegmentProfile::Segmented32,
                    &code,
                    &image,
                    &[],
                    Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit,
                    },
                );
            }
        }
    }
}

#[test]
fn the_complete_pointer_segment_check_precedes_paging_and_resolution() {
    source_faults(Engine::Wasmtime);
}

fn resolver_faults(engine: Engine) {
    for (segment, opcode, selector, present_slot, exit) in [
        (
            Segment::Ds,
            &[0xc5][..],
            0xf327,
            false,
            Exit::GeneralProtection { error: 0xf324 },
        ),
        (
            Segment::Ds,
            &[0xc5][..],
            0xf327,
            true,
            Exit::Other(0x0020_f324_0000_0000),
        ),
        (
            Segment::Ss,
            &[0x0f, 0xb2][..],
            0xf327,
            true,
            Exit::StackFault { error: 0xf324 },
        ),
        (
            Segment::Ss,
            &[0x0f, 0xb2][..],
            3,
            false,
            Exit::GeneralProtection { error: 0 },
        ),
    ] {
        let mut code = opcode.to_vec();
        code.push(0x23); // Destination ESP, source [EBX].
        let mut image = Image::new(&code);
        image.cpu.registers.ebx = 0x4000;
        image.map(4, 0x8000, false);
        image.data(0x8000, &[0x78, 0x56, 0x34, 0x92]);
        image.data(0x8004, &u16::to_le_bytes(selector));
        let mut tables = DescriptorTables::default();
        if present_slot {
            tables.insert(
                selector,
                SegmentDescriptor {
                    present: false,
                    ..descriptor(0x9000, SegmentDefaultSize::Bits16)
                },
            );
        }
        check_one(
            engine,
            SegmentProfile::Flat32,
            &code,
            &image,
            &[SegmentResolution::new(&tables, segment, selector)],
            Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            },
        );
    }
}

#[test]
fn failed_resolution_preserves_the_destination_gpr_and_complete_segment_cache() {
    resolver_faults(Engine::Wasmtime);
}

fn earlier_progress(engine: Engine) {
    // ADD EAX,1; MOV [EBX],EAX; LSS ESP,[EBX] (non-present).
    let code = [0x83, 0xc0, 1, 0x89, 0x03, 0x0f, 0xb2, 0x23];
    let mut image = Image::new(&code);
    image.cpu.registers.eax = 1;
    image.cpu.registers.ebx = 0x4000;
    image.map(4, 0x8000, true);
    image.data(0x8004, &[0x27, 0]);
    let mut tables = DescriptorTables::default();
    tables.insert(
        0x27,
        SegmentDescriptor {
            present: false,
            ..descriptor(0x9000, SegmentDefaultSize::Bits16)
        },
    );
    let mut input = image.input();
    input
        .segment_resolutions
        .push(SegmentResolution::new(&tables, Segment::Ss, 0x27));
    let mut cpu = image.cpu;
    cpu.registers.eax = 2;
    cpu.flags.status_source.kind = 10;
    cpu.flags.status_source.left = 1;
    cpu.flags.status_source.right = 1;
    cpu.eip = 0x1003;
    cpu.instruction_count = 0;
    let first = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    };
    cpu.eip = 0x1005;
    cpu.instruction_count = 1;
    let ram: &[(u32, &[u8])] = &[(0x8000, &[2, 0, 0, 0])];
    let second = Step {
        cpu,
        ram,
        exit: Exit::Dispatch(cpu.eip),
    };
    let fault = || Step {
        cpu,
        ram,
        exit: Exit::StackFault { error: 0x24 },
    };
    let event = Event::ResolveSegment {
        segment: 2,
        selector: 0x27,
    };
    let mut wanted = expected(&image, &[first, second, fault()]);
    wanted.events.insert(4, event.clone());
    assert_eq!(engine.observe(TestModule::interpreter(), &input, 3), wanted);
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 3, SegmentProfile::Flat32);
    let mut wanted = expected(&image, &[fault()]);
    wanted.events.insert(0, event);
    assert_eq!(engine.observe(block, &input, 1), wanted);
}

#[test]
fn pointer_load_faults_publish_earlier_flags_register_store_and_retirement_progress() {
    earlier_progress(Engine::Wasmtime);
}

fn register_sources(engine: Engine) {
    for (_, opcode) in FORMS {
        let rm = 3;
        let mut code = opcode.to_vec();
        code.push(0xc0 | rm);
        let mut image = Image::empty();
        image.cpu.eip = 0x2000 - code.len() as u32;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, &code);
        assert!(matches!(
            wasm86_x86::compile_block_from_bytes(image.cpu.eip, &code, 1),
            Err(wasm86_x86::BlockError::UnsupportedInstruction { .. })
        ));
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("pointer load rejects register source {code:02x?}"),
            Exit::Other((8 << 48) | (u64::from(opcode[0]) << 32) | u64::from(image.cpu.eip)),
        );
    }
}

#[test]
fn register_sources_are_rejected_by_both_decoders_before_execution() {
    register_sources(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_pointer_load_fault_publication_and_invalid_forms() {
    source_faults(Engine::V8);
    resolver_faults(Engine::V8);
    earlier_progress(Engine::V8);
    register_sources(Engine::V8);
}
