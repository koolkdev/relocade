use super::*;

fn stack_load(engine: Engine) {
    for usable_pointer in [false, true] {
        let code = [0x0f, 0xb2, 0x23, 0x51]; // LSS ESP,[EBX]; PUSH ECX.
        let mut image = Image::new(&code);
        image.cpu.registers.ebx = 0x4000;
        image.cpu.registers.ecx = 0x1234_5678;
        image.map(4, 0x8000, false);
        image.map(0x18, 0xc000, true);
        let offset: u32 = if usable_pointer { 0x10000 } else { 0x108 };
        image.data(0x8000, &offset.to_le_bytes());
        image.data(0x8004, &[0x27, 0]);
        let limit = if usable_pointer { 0xffff } else { 0xff };
        let mut tables = DescriptorTables::default();
        tables.insert(
            0x27,
            SegmentDescriptor {
                limit: crate::SegmentLimit::from_effective(limit).unwrap(),
                ..descriptor(0x9000, SegmentDefaultSize::Bits16)
            },
        );
        let mut cpu = image.cpu;
        cpu.registers.esp = offset;
        cpu.segments.ss = loaded(0x27, 0x9000, limit);
        cpu.eip = 0x1003;
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
        // LSS admits either offset. The next PUSH applies the new SS.B and limit.
        image.cpu = cpu;
        if usable_pointer {
            cpu.registers.esp = 0x1fffc;
            cpu.eip = 0x1004;
            cpu.instruction_count = 1;
        }
        check_one(
            engine,
            SegmentProfile::Segmented32,
            &code[3..],
            &image,
            &[],
            Step {
                cpu,
                ram: if usable_pointer {
                    &[(0xcffc, &[0x78, 0x56, 0x34, 0x12])]
                } else {
                    &[]
                },
                exit: if usable_pointer {
                    Exit::Dispatch(cpu.eip)
                } else {
                    Exit::StackFault { error: 0 }
                },
            },
        );
    }
}

#[test]
fn lss_commits_the_offset_before_the_next_entry_applies_the_new_stack_rules() {
    stack_load(Engine::Wasmtime);
}

fn terminal_loads(engine: Engine) {
    for (segment, opcode) in FORMS {
        let mut code = opcode.to_vec();
        code.extend([0x03, 0x62]); // Load EAX,[EBX], followed by an unsupported byte.
        let mut image = Image::new(&code);
        image.cpu.registers.ebx = 0x4000;
        image.map(4, 0x8000, false);
        image.data(0x8000, &[0x78, 0x56, 0x34, 0x92, 0x27, 0]);
        let mut tables = DescriptorTables::default();
        tables.insert(0x27, descriptor(0x9000, SegmentDefaultSize::Bits16));
        let mut input = image.input();
        input
            .segment_resolutions
            .push(SegmentResolution::new(&tables, segment, 0x27));
        let mut cpu = image.cpu;
        cpu.registers.eax = 0x9234_5678;
        cpu.segments[segment] = loaded(0x27, 0x9000, 0xffff);
        cpu.eip += opcode.len() as u32 + 1;
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
fn all_pointer_loads_end_the_block_after_committing_both_results() {
    terminal_loads(Engine::Wasmtime);
}

fn null_and_fetch_boundary(engine: Engine) {
    let mut code = vec![0x67; 12];
    code.extend([0x0f, 0xb4, 0]); // Fifteen-byte LFS EAX,[BX+SI].
    let mut image = Image::empty();
    image.cpu.eip = 0x1ff1;
    image.cpu.registers.ebx = 0x4000;
    image.cpu.registers.esi = 0;
    image.map(1, 0x3000, false);
    image.map(4, 0x8000, false);
    image.data(0x3ff1, &code);
    image.data(0x8000, &[0x78, 0x56, 0x34, 0x92, 3, 0]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x9234_5678;
    cpu.segments.fs = StoredSegment::unusable(3);
    cpu.eip = 0x2000;
    cpu.instruction_count = 0;
    check_one(
        engine,
        SegmentProfile::Flat32,
        &code,
        &image,
        &[SegmentResolution::new(
            &DescriptorTables::default(),
            Segment::Fs,
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
fn null_selector_loads_still_commit_the_offset_without_fetching_a_following_byte() {
    null_and_fetch_boundary(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_pointer_load_dispatch_and_stack_continuation() {
    stack_load(Engine::V8);
    terminal_loads(Engine::V8);
    null_and_fetch_boundary(Engine::V8);
}
