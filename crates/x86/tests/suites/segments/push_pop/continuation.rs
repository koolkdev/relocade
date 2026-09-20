use super::*;

fn changed_stack_width(engine: Engine) {
    for old_big in [false, true] {
        let code = [0x17, 0x51]; // POP SS; PUSH ECX.
        let mut image = Image::new(&code);
        image.cpu.segments.ss = data(0x4000, 0xffff);
        image.cpu.segments.ss.attributes = stack_attributes(old_big);
        image.cpu.registers.esp = if old_big { 0xfffc } else { 0xabcd_fffc };
        image.cpu.registers.ecx = 0x1234_5678;
        image.map(0x13, 0x8000, false);
        image.data(0x8ffc, &[0x27, 0, 0xa5, 0x5a]);
        // After changing B, the next instruction must use the newly loaded base
        // and the opposite pointer rule. Map only its independently derived address.
        let (base, limit, next_page) = if old_big {
            (0x9000, 0xffff, 0x18)
        } else {
            (0x1000, u32::MAX, 0xabcd0)
        };
        image.map(next_page, 0xc000, true);
        let mut tables = DescriptorTables::default();
        tables.insert(
            0x27,
            SegmentDescriptor {
                limit: crate::SegmentLimit::from_effective(limit).unwrap(),
                ..descriptor(
                    base,
                    if old_big {
                        SegmentDefaultSize::Bits16
                    } else {
                        SegmentDefaultSize::Bits32
                    },
                )
            },
        );
        let mut cpu = image.cpu;
        cpu.registers.esp = if old_big { 0x10000 } else { 0xabcd_0000 };
        cpu.segments.ss = StoredSegment {
            base,
            limit,
            selector: 0x27,
            attributes: SegmentAttributes::from_bits(if old_big { 5 } else { 0x15 }),
        };
        cpu.eip = 0x1001;
        cpu.instruction_count = 0;
        check_one(
            engine,
            SegmentProfile::Segmented32,
            &code,
            &image,
            &[SegmentResolution::new(&tables, Segment::Ss, 0x27)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
        image.cpu = cpu;
        cpu.registers.esp = if old_big { 0x1fffc } else { 0xabcc_fffc };
        cpu.eip = 0x1002;
        cpu.instruction_count = 1;
        check_one(
            engine,
            SegmentProfile::Segmented32,
            &code[1..],
            &image,
            &[],
            Step {
                cpu,
                ram: &[(0xcffc, &[0x78, 0x56, 0x34, 0x12])],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }
}

#[test]
fn pop_ss_advances_with_old_stack_width_and_the_next_entry_uses_the_new_cache() {
    changed_stack_width(Engine::Wasmtime);
}

fn terminal_pops(engine: Engine) {
    for (segment, opcode) in POP {
        let mut code = opcode.to_vec();
        code.push(0xf4); // Unsupported next byte must remain undecoded.
        let mut image = Image::new(&code);
        image.cpu.registers.esp = 0x4000;
        image.map(4, 0x8000, false);
        image.data(0x8000, &[0x27, 0]);
        let mut tables = DescriptorTables::default();
        tables.insert(0x27, descriptor(0x9000, SegmentDefaultSize::Bits16));
        let mut input = image.input();
        input
            .segment_resolutions
            .push(SegmentResolution::new(&tables, segment, 0x27));
        let mut cpu = image.cpu;
        cpu.registers.esp = 0x4004;
        cpu.segments[segment] = StoredSegment {
            base: 0x9000,
            limit: 0xffff,
            selector: 0x27,
            attributes: SegmentAttributes::from_bits(5),
        };
        cpu.eip += opcode.len() as u32;
        cpu.instruction_count = 0;
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
        wanted.events.insert(
            0,
            Event::ResolveSegment {
                segment: segment as i32,
                selector: 0x27,
            },
        );
        let mut blocks = BlockModules::default();
        for module in [
            blocks.get(&image.cpu, &code, 2, SegmentProfile::Flat32),
            TestModule::interpreter(),
        ] {
            assert_eq!(engine.observe(module, &input, 1), wanted);
        }
    }
}

#[test]
fn every_segment_pop_ends_the_block_at_the_cache_commit_boundary() {
    terminal_pops(Engine::Wasmtime);
}

fn stack_roundtrip(engine: Engine) {
    // PUSH DS can continue into POP ES. Only the POP resolves a descriptor.
    let code = [0x1e, 0x07];
    let mut image = Image::new(&code);
    image.cpu.segments.ds.selector = 3;
    image.cpu.registers.esp = 0x4104;
    image.map(4, 0x8000, true);
    image.data(0x8100, &[0xa5; 4]);
    let mut input = image.input();
    input.segment_resolutions.push(SegmentResolution::new(
        &DescriptorTables::default(),
        Segment::Es,
        3,
    ));
    let mut cpu = image.cpu;
    cpu.registers.esp = 0x4100;
    cpu.eip = 0x1001;
    cpu.instruction_count = 0;
    let ram: &[(u32, &[u8])] = &[(0x8100, &[3, 0])];
    let first = Step {
        cpu,
        ram,
        exit: Exit::Dispatch(cpu.eip),
    };
    cpu.registers.esp = 0x4104;
    cpu.segments.es = StoredSegment::unusable(3);
    cpu.eip = 0x1002;
    cpu.instruction_count = 1;
    assert!(!SegmentProfile::Flat32.is_compatible_with(&cpu.segments));
    let last = || Step {
        cpu,
        ram,
        exit: Exit::Dispatch(cpu.eip),
    };
    let event = Event::ResolveSegment {
        segment: 0,
        selector: 3,
    };
    let mut wanted = expected(&image, &[first, last()]);
    wanted.events.insert(2, event.clone());
    assert_eq!(engine.observe(TestModule::interpreter(), &input, 2), wanted);
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 2, SegmentProfile::Flat32);
    let mut wanted = expected(&image, &[last()]);
    wanted.events.insert(0, event);
    assert_eq!(engine.observe(block, &input, 1), wanted);
}

#[test]
fn push_can_continue_into_pop_and_a_null_cache_is_published_before_dispatch() {
    stack_roundtrip(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_segment_stack_dispatch_and_continuation() {
    changed_stack_width(Engine::V8);
    terminal_pops(Engine::V8);
    stack_roundtrip(Engine::V8);
}
