use super::*;

fn resolver_faults(engine: Engine) {
    for (segment, selector, present_slot, exit) in [
        (
            Segment::Ds,
            0xf327,
            false,
            Exit::GeneralProtection { error: 0xf324 },
        ),
        (
            Segment::Ds,
            0xf327,
            true,
            Exit::Other(0x0020_f324_0000_0000),
        ),
        (
            Segment::Ss,
            0xf327,
            true,
            Exit::StackFault { error: 0xf324 },
        ),
        (Segment::Ss, 3, false, Exit::GeneralProtection { error: 0 }),
    ] {
        let code = [0x8e, 0xc0 | ((segment as u8) << 3)];
        let mut image = Image::new(&code);
        image.cpu.registers.eax = 0xabcd_0000 | u32::from(selector);
        let mut tables = DescriptorTables::default();
        if present_slot {
            tables.insert(
                selector,
                SegmentDescriptor {
                    present: false,
                    ..descriptor(0x8000, SegmentDefaultSize::Bits32)
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
fn failed_resolution_preserves_all_cpu_state_and_reports_the_shared_fault() {
    resolver_faults(Engine::Wasmtime);
}

fn source_faults(engine: Engine) {
    for segment_denied in [false, true] {
        let code = [0x8e, 0x14, 0x24]; // MOV SS,[ESP].
        let mut image = Image::new(&code);
        image.cpu.registers.esp = 0xfff;
        image.cpu.segments.ss = data(0x4000, if segment_denied { 0xfff } else { 0xffff });
        image.map(4, 0x8000, false);
        image.data(0x8fff, &[0x27]);
        let exit = if segment_denied {
            Exit::StackFault { error: 0 }
        } else {
            Exit::PageFault {
                address: 0x5000,
                error: 0,
            }
        };
        // An empty response script also proves the resolver was never called.
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
    let code = [0x8c, 0x03]; // MOV [EBX],ES across a read-only second page.
    let mut image = Image::new(&code);
    image.cpu.registers.ebx = 0x4fff;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, false);
    image.data(0x8fff, &[0xa5]);
    image.data(0xa000, &[0x5a]);
    check_one(
        engine,
        SegmentProfile::Flat32,
        &code,
        &image,
        &[],
        Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x5000,
                error: 3,
            },
        },
    );
}

#[test]
fn operand_faults_precede_resolution_and_split_stores_do_not_partially_write() {
    source_faults(Engine::Wasmtime);
}

fn earlier_progress(engine: Engine) {
    // MOV EAX,1; ADD EAX,1; MOV [EBX],EAX; MOV SS,ECX (non-present).
    let code = [0xb8, 1, 0, 0, 0, 0x83, 0xc0, 1, 0x89, 0x03, 0x8e, 0xd1];
    let mut image = Image::new(&code);
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.ecx = 0xabcd_0027;
    image.map(4, 0x8000, true);
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
    cpu.registers.eax = 1;
    cpu.eip = 0x1005;
    cpu.instruction_count = 0;
    let first = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    };
    cpu.registers.eax = 2;
    cpu.flags.status_source.kind = 10;
    cpu.flags.status_source.left = 1;
    cpu.flags.status_source.right = 1;
    cpu.eip = 0x1008;
    cpu.instruction_count = 1;
    let second = Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    };
    cpu.eip = 0x100a;
    cpu.instruction_count = 2;
    let ram: &[(u32, &[u8])] = &[(0x8000, &[2, 0, 0, 0])];
    let third = Step {
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
    let mut wanted = expected(&image, &[first, second, third, fault()]);
    wanted.events.insert(6, event.clone());
    assert_eq!(engine.observe(TestModule::interpreter(), &input, 4), wanted);
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 4, SegmentProfile::Flat32);
    let mut wanted = expected(&image, &[fault()]);
    wanted.events.insert(0, event);
    assert_eq!(engine.observe(block, &input, 1), wanted);
}

#[test]
fn a_resolver_fault_publishes_earlier_register_flags_store_and_retirement_progress() {
    earlier_progress(Engine::Wasmtime);
}

fn invalid_extensions(engine: Engine) {
    for (opcode, extension) in [(0x8c, 6), (0x8c, 7), (0x8e, 1), (0x8e, 6), (0x8e, 7)] {
        for mode in [0, 3] {
            // Memory mode needs an unmapped disp32 after the ModRM byte.
            let code = [opcode, (mode << 6) | (extension << 3) | 5];
            assert!(matches!(
                wasm86_x86::compile_block_from_bytes(0x1ffe, &code, 1),
                Err(wasm86_x86::BlockError::UnsupportedInstruction { .. })
            ));
            let mut image = Image::empty();
            image.cpu.eip = 0x1ffe;
            image.map(1, 0x3000, false);
            image.data(0x3ffe, &code);
            assert_eq!(
                engine.observe(TestModule::interpreter(), &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: Exit::Other((8 << 48) | (u64::from(opcode) << 32) | 0x1ffe),
                    }]
                ),
            );
        }
    }
}

#[test]
fn invalid_segment_encodings_are_rejected_before_address_bytes_or_source_access() {
    invalid_extensions(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_segment_fault_ordering_restart_and_invalid_extensions() {
    resolver_faults(Engine::V8);
    source_faults(Engine::V8);
    earlier_progress(Engine::V8);
    invalid_extensions(Engine::V8);
}
