use super::*;

fn changed_stack_width(engine: Engine) {
    let code = [0x8e, 0xd0, 0x51]; // MOV SS,EAX; PUSH ECX.
    let mut image = Image::new(&code);
    image.cpu.registers.eax = 0x27;
    image.cpu.registers.ecx = 0x1234_5678;
    image.cpu.registers.esp = 0xabcd_0000;
    image.map(0x13, 0x8000, true);
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, descriptor(0x4000, SegmentDefaultSize::Bits16));
    let mut cpu = image.cpu;
    cpu.segments.ss = StoredSegment {
        base: 0x4000,
        limit: 0xffff,
        selector: 0x27,
        attributes: SegmentAttributes::from_bits(5),
    };
    cpu.eip = 0x1002;
    cpu.instruction_count = 0;
    check_one(
        engine,
        SegmentProfile::Flat32,
        &code,
        &image,
        &[SegmentResolution::new(&tables, Segment::Ss, 0x27)],
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
    // Resume the published state under a compatible profile. SS.B controls SP
    // wrapping independently of the still-32-bit operand and code defaults.
    image.cpu = cpu;
    cpu.registers.esp = 0xabcd_fffc;
    cpu.eip = 0x1003;
    cpu.instruction_count = 1;
    check_one(
        engine,
        SegmentProfile::Segmented32,
        &code[2..],
        &image,
        &[],
        Step {
            cpu,
            ram: &[(0x8ffc, &[0x78, 0x56, 0x34, 0x12])],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

#[test]
fn the_next_entry_uses_loaded_ss_base_and_stack_width() {
    changed_stack_width(Engine::Wasmtime);
}

fn terminal_null_load(engine: Engine) {
    // MOV DS,EAX must end compilation before the unsupported following byte.
    let code = [0x8e, 0xd8, 0xf4];
    let mut image = Image::new(&code);
    image.cpu.registers.eax = 0xabcd_0003;
    let tables = DescriptorTables::default();
    let mut input = image.input();
    input
        .segment_resolutions
        .push(SegmentResolution::new(&tables, Segment::Ds, 3));
    let mut cpu = image.cpu;
    cpu.segments.ds = StoredSegment::unusable(3);
    cpu.eip += 2;
    cpu.instruction_count = 0;
    assert!(!SegmentProfile::Flat32.is_compatible_with(&cpu.segments));
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
            segment: 3,
            selector: 3,
        },
    );
    let mut blocks = BlockModules::default();
    for module in [
        blocks.get(&image.cpu, &code, 2, SegmentProfile::Flat32),
        TestModule::interpreter(),
    ] {
        assert_eq!(engine.observe(module, &input, 1), wanted);
    }

    // The dispatch owner can admit the resulting cache under Segmented32.
    let read = [0x8c, 0xd8]; // MOV EAX,DS remains legal with unusable DS.
    let mut next = Image::new(&read);
    next.cpu = cpu;
    next.cpu.eip = 0x1000;
    cpu = next.cpu;
    cpu.registers.eax = 3;
    cpu.eip += 2;
    cpu.instruction_count = 1;
    check_one(
        engine,
        SegmentProfile::Segmented32,
        &read,
        &next,
        &[],
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

#[test]
fn a_successful_load_can_break_flat_assumptions_only_at_the_dispatch_boundary() {
    terminal_null_load(Engine::Wasmtime);
}

fn repeated_reload(engine: Engine) {
    // MOV FS,EAX; MOV ECX,FS:[EBX]; MOV FS,EAX; MOV ECX,FS:[EBX].
    let code = [0x8e, 0xe0, 0x64, 0x8b, 0x0b, 0x8e, 0xe0, 0x64, 0x8b, 0x0b];
    let mut image = Image::new(&code);
    image.cpu.registers.eax = 0xabcd_0027;
    image.cpu.registers.ebx = 0x20;
    image.map(4, 0x8000, false);
    image.map(5, 0x9000, false);
    image.data(0x8020, &[0x11; 4]);
    image.data(0x9020, &[0x22; 4]);
    let mut tables = DescriptorTables::default();
    let mut input = image.input();
    for base in [0x4000, 0x5000] {
        tables.insert(0x27, descriptor(base, SegmentDefaultSize::Bits32));
        input
            .segment_resolutions
            .push(SegmentResolution::new(&tables, Segment::Fs, 0x27));
    }
    let mut cpu = image.cpu;
    let mut steps = Vec::new();
    for (base, value) in [(0x4000, 0x1111_1111), (0x5000, 0x2222_2222)] {
        cpu.segments.fs = StoredSegment {
            base,
            limit: 0xffff,
            selector: 0x27,
            attributes: SegmentAttributes::from_bits(0x15),
        };
        cpu.eip += 2;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        steps.push(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        });
        cpu.registers.ecx = value;
        cpu.eip += 3;
        cpu.instruction_count += 1;
        steps.push(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        });
    }
    let mut wanted = expected(&image, &steps);
    wanted.events.insert(
        4,
        Event::ResolveSegment {
            segment: 4,
            selector: 0x27,
        },
    );
    wanted.events.insert(
        0,
        Event::ResolveSegment {
            segment: 4,
            selector: 0x27,
        },
    );
    assert_eq!(engine.observe(TestModule::interpreter(), &input, 4), wanted);
}

#[test]
fn reloading_the_same_selector_refreshes_the_cache_for_later_entries() {
    repeated_reload(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_segment_load_dispatch_and_continuation() {
    changed_stack_width(Engine::V8);
    terminal_null_load(Engine::V8);
    repeated_reload(Engine::V8);
}
