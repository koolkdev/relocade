use super::*;

fn access_faults(engine: Engine) {
    for push in [false, true] {
        for (limit, second_page, exit) in [
            (0xfff, None, Exit::StackFault { error: 0 }),
            (
                0xffff,
                None,
                Exit::PageFault {
                    address: 0x5000,
                    error: if push { 2 } else { 0 },
                },
            ),
            (
                0xffff,
                Some(false),
                Exit::PageFault {
                    address: 0x5000,
                    error: 3,
                },
            ),
        ] {
            if second_page.is_some() && !push {
                continue;
            }
            let code = [if push { 0x16 } else { 0x17 }]; // PUSH/POP SS.
            let mut image = Image::new(&code);
            image.cpu.segments.ss = data(0x4000, limit);
            image.cpu.registers.esp = if push { 0x1003 } else { 0xfff };
            image.map(4, 0x8000, true);
            image.data(0x8fff, &[0x27]);
            if let Some(writable) = second_page {
                image.map(5, 0xa000, writable);
                image.data(0xa000, &[0xf3]);
            }
            // No resolver response: faults must precede the host call and every write.
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

#[test]
fn selector_access_faults_preserve_esp_cache_and_both_halves_of_split_stores() {
    access_faults(Engine::Wasmtime);
}

fn resolution_faults(engine: Engine) {
    for (segment, opcode, selector, nonpresent, exit) in [
        (
            Segment::Ds,
            0x1f,
            0xf327,
            false,
            Exit::GeneralProtection { error: 0xf324 },
        ),
        (
            Segment::Ds,
            0x1f,
            0xf327,
            true,
            Exit::Other(0x0020_f324_0000_0000),
        ),
        (
            Segment::Ss,
            0x17,
            0xf327,
            true,
            Exit::StackFault { error: 0xf324 },
        ),
        (
            Segment::Ss,
            0x17,
            3,
            false,
            Exit::GeneralProtection { error: 0 },
        ),
    ] {
        let code = [opcode];
        let mut image = Image::new(&code);
        image.cpu.registers.esp = 0x4000;
        image.map(4, 0x8000, false);
        image.data(0x8000, &u16::to_le_bytes(selector));
        let mut tables = DescriptorTables::default();
        if nonpresent {
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
fn a_failed_pop_resolution_does_not_commit_the_stack_advance() {
    resolution_faults(Engine::Wasmtime);
}

fn earlier_progress(engine: Engine) {
    // ADD EAX,1; MOV [EBX],EAX; POP SS (non-present).
    let code = [0x83, 0xc0, 1, 0x89, 0x03, 0x17];
    let mut image = Image::new(&code);
    image.cpu.registers.eax = 1;
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.esp = 0x4ffe;
    image.map(4, 0x8000, true);
    image.data(0x8ffe, &[0x27, 0]);
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
fn a_pop_fault_publishes_earlier_register_flags_store_and_retirement_progress() {
    earlier_progress(Engine::Wasmtime);
}

fn opcode_fetch(engine: Engine) {
    for prefixes in [0, 13, 14] {
        let mut code = vec![0x66; prefixes];
        code.push(0x0f);
        let mut image = Image::empty();
        image.cpu.eip = 0x2000 - code.len() as u32;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, &code);
        let exit = if prefixes == 14 {
            Exit::GeneralProtection { error: 0 }
        } else {
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            }
        };
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("segment stack opcode fetch with {prefixes} prefixes"),
            exit,
        );
    }
    // A complete 15-byte POP GS does not fetch byte 16 before dispatching.
    let mut code = vec![0x66; 13];
    code.extend([0x0f, 0xa9]);
    let mut image = Image::empty();
    image.cpu.eip = 0x1ff1;
    image.cpu.registers.esp = 0x4000;
    image.map(1, 0x3000, false);
    image.map(4, 0x8000, false);
    image.data(0x3ff1, &code);
    image.data(0x8000, &[3, 0]);
    let mut cpu = image.cpu;
    cpu.segments.gs = StoredSegment::unusable(3);
    cpu.registers.esp = 0x4002;
    cpu.eip = 0x2000;
    cpu.instruction_count = 0;
    check_one(
        engine,
        SegmentProfile::Flat32,
        &code,
        &image,
        &[SegmentResolution::new(
            &DescriptorTables::default(),
            Segment::Gs,
            3,
        )],
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

#[test]
fn extended_segment_opcodes_obey_fetch_fault_and_instruction_length_boundaries() {
    opcode_fetch(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_segment_stack_fault_ordering_and_restart() {
    access_faults(Engine::V8);
    resolution_faults(Engine::V8);
    earlier_progress(Engine::V8);
    opcode_fetch(Engine::V8);
}
