use super::*;
use crate::SegmentProfile;

#[test]
fn the_next_entry_uses_loaded_ss_base_and_stack_width() {
    let code = [0x8e, 0xd0, 0x51]; // MOV SS,EAX; PUSH ECX.
    let mut image = Image::new(&code);
    image.cpu.registers.eax = 0x27;
    image.cpu.registers.ecx = 0x1234_5678;
    image.cpu.registers.esp = 0xabcd_0000;
    image.map(0x13, 0x8000, true);
    let mut tables = DescriptorTables::default();
    tables.insert(0x27, descriptor(0x4000, SegmentDefaultSize::Bits16));
    let mut cpu = image.cpu;
    cpu.segments.ss = SegmentRegister {
        base: 0x4000,
        limit: 0xffff,
        selector: 0x27,
        big: 0,
        ..SegmentRegister::flat_data32(0)
    };
    cpu.eip = 0x1002;
    cpu.instruction_count = 0;
    Scenario::new(
        ExecutionProfile::Protected(SegmentProfile::Flat32),
        &code,
        &image,
    )
    .resolve(
        &[SegmentResolution::new(&tables, Segment::Ss, 0x27)],
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    )
    .check();
    // Resume the published state under a compatible profile. SS.B controls SP
    // wrapping independently of the still-32-bit operand and code defaults.
    image.cpu = cpu;
    cpu.registers.esp = 0xabcd_fffc;
    cpu.eip = 0x1003;
    cpu.instruction_count = 1;
    Scenario::new(
        ExecutionProfile::Protected(SegmentProfile::Segmented32),
        &code[2..],
        &image,
    )
    .step(Step {
        cpu,
        ram: &[(0x8ffc, &[0x78, 0x56, 0x34, 0x12])],
        exit: Exit::Dispatch(cpu.eip),
    })
    .check();
}

#[test]
fn a_successful_load_can_break_flat_assumptions_only_at_the_dispatch_boundary() {
    // MOV DS,EAX must end compilation before the unsupported following byte.
    let code = [0x8e, 0xd8, 0xf4];
    let mut image = Image::new(&code);
    image.cpu.registers.eax = 0xabcd_0003;
    let tables = DescriptorTables::default();
    let mut cpu = image.cpu;
    cpu.segments.ds = SegmentRegister::unusable(3);
    cpu.eip += 2;
    cpu.instruction_count = 0;
    assert!(!ExecutionProfile::Protected(SegmentProfile::Flat32).is_compatible_with(&cpu.segments));
    let mut blocks = BlockModules::default();
    Scenario::new(
        ExecutionProfile::Protected(SegmentProfile::Flat32),
        &code,
        &image,
    )
    .resolve(
        &[SegmentResolution::new(&tables, Segment::Ds, 3)],
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    )
    .check_with_block(blocks.get(
        &image.cpu,
        &code,
        2,
        ExecutionProfile::Protected(SegmentProfile::Flat32),
    ));

    // The dispatch owner can admit the resulting cache under Segmented32.
    let read = [0x8c, 0xd8]; // MOV EAX,DS remains legal with unusable DS.
    let mut next = Image::new(&read);
    next.cpu = cpu;
    next.cpu.eip = 0x1000;
    cpu = next.cpu;
    cpu.registers.eax = 3;
    cpu.eip += 2;
    cpu.instruction_count = 1;
    Scenario::new(
        ExecutionProfile::Protected(SegmentProfile::Segmented32),
        &read,
        &next,
    )
    .step(Step {
        cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    })
    .check();
}

#[test]
fn reloading_the_same_selector_refreshes_the_cache_for_later_entries() {
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
    let mut scenario = Scenario::new(
        ExecutionProfile::Protected(SegmentProfile::Flat32),
        &code,
        &image,
    );
    let mut cpu = image.cpu;
    for (base, value) in [(0x4000, 0x1111_1111), (0x5000, 0x2222_2222)] {
        tables.insert(0x27, descriptor(base, SegmentDefaultSize::Bits32));
        cpu.segments.fs = SegmentRegister {
            base,
            limit: 0xffff,
            selector: 0x27,
            ..SegmentRegister::flat_data32(0)
        };
        cpu.eip += 2;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        scenario = scenario.resolve(
            &[SegmentResolution::new(&tables, Segment::Fs, 0x27)],
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
        cpu.registers.ecx = value;
        cpu.eip += 3;
        cpu.instruction_count += 1;
        scenario = scenario.step(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        });
    }
    scenario.check_interpreter();
}
