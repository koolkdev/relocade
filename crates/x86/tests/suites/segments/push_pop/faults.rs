use super::*;
use crate::SegmentProfile;

#[test]
fn selector_access_faults_preserve_esp_cache_and_both_halves_of_split_stores() {
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
            Scenario::new(
                ExecutionProfile::Protected(SegmentProfile::Segmented32),
                &code,
                &image,
            )
            .step(Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            })
            .check();
        }
    }
}

#[test]
fn a_failed_pop_resolution_does_not_commit_the_stack_advance() {
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
        Scenario::new(
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            &code,
            &image,
        )
        .resolve(
            &[SegmentResolution::new(&tables, segment, selector)],
            Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            },
        )
        .check();
    }
}

#[test]
fn a_pop_fault_publishes_earlier_register_flags_store_and_retirement_progress() {
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
    let fault = Step {
        cpu,
        ram: &[],
        exit: Exit::StackFault { error: 0x24 },
    };
    Scenario::new(
        ExecutionProfile::Protected(SegmentProfile::Flat32),
        &code,
        &image,
    )
    .step(first)
    .step(second)
    .resolve(&[SegmentResolution::new(&tables, Segment::Ss, 0x27)], fault)
    .check();
}

#[test]
fn extended_segment_opcodes_obey_fetch_fault_and_instruction_length_boundaries() {
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
    cpu.segments.gs = SegmentRegister::unusable(3);
    cpu.registers.esp = 0x4002;
    cpu.eip = 0x2000;
    cpu.instruction_count = 0;
    Scenario::new(
        ExecutionProfile::Protected(SegmentProfile::Flat32),
        &code,
        &image,
    )
    .resolve(
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
    )
    .check();
}
