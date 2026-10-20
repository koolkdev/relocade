use super::*;
use crate::SegmentProfile;
use relocade_x86::SegmentRegister;

#[test]
fn an_unsupported_or_faulting_return_publishes_only_earlier_completed_instructions() {
    for nested_task in [false, true] {
        // ADD EAX,1; prefixed IRETD; unsupported trailing byte beyond the block end.
        let code = [0x83, 0xc0, 1, 0x67, 0xcf, 0xf4];
        let profile = ExecutionProfile::Protected(SegmentProfile::Flat32);
        let mut image = image(&code, profile);
        image.cpu.flags.status_source.kind = 0;
        image.cpu.flags.status_source.left = 0;
        image.cpu.flags.status_source.right = 0;
        image.cpu.flags.bytes.nt = u8::from(nested_task);
        image.cpu.registers.eax = 0x7fff_ffff;
        image.cpu.registers.esp = 0x4000;
        let mut cpu = image.cpu;
        cpu.registers.eax = 0x8000_0000;
        cpu.eip = 0x1003;
        cpu.instruction_count = 0;
        cpu.flags.status_source.kind = 10;
        cpu.flags.status_source.left = 0x7fff_ffff;
        cpu.flags.status_source.right = 1;
        let fault = || Step {
            cpu,
            ram: &[],
            exit: if nested_task {
                Exit::Other(0x0008_00cf_0000_1003)
            } else {
                Exit::PageFault {
                    address: 0x4000,
                    error: 0,
                }
            },
        };
        assert_eq!(
            TestModule::interpreter().observe(&image.input(), 2),
            expected(
                &image,
                &[
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Dispatch(cpu.eip)
                    },
                    fault(),
                ]
            )
        );
        let mut blocks = BlockModules::default();
        let block = blocks.get(&image.cpu, &code, 3, profile);
        assert_eq!(
            block.observe(&image.input(), 1),
            expected(&image, &[fault()])
        );
    }
}

#[test]
fn returned_cs_and_nt_apply_at_the_next_entry_after_retirement() {
    let code = [0xcf, 0xf4];
    let profile = ExecutionProfile::Protected(SegmentProfile::Segmented32);
    let mut image = image(&code, profile);
    image.cpu.segments.cs.limit = 0x1000;
    image.data(0x8000, &frame(false, 0x200, 0x27, 0x4000));
    let mut tables = tables(0xffff);
    tables.insert(0x27, descriptor(0xc000, 0xffff, SegmentDefaultSize::Bits16));
    let mut input = image.input();
    input
        .segment_resolutions
        .push(SegmentResolution::new(&tables, Segment::Cs, 0x27));
    let mut cpu = cleared_flags(image.cpu, false);
    cpu.flags.bytes.nt = 1;
    cpu.segments.cs = loaded(
        0x27,
        0xc000,
        0xffff,
        SegmentRegister {
            big: 0,
            ..SegmentRegister::flat_code32(0)
        },
    );
    cpu.eip = 0x200;
    cpu.registers.esp = 0x900c;
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
            segment: 1,
            selector: 0x27,
        },
    );
    let mut blocks = BlockModules::default();
    for module in [
        blocks.get(&image.cpu, &code, 2, profile),
        TestModule::interpreter_with_profile(profile),
    ] {
        assert_eq!(module.observe(&input, 1), wanted);
    }
    image.cpu = cpu;
    let step = TestModule::interpreter_with_profile(ExecutionProfile::Protected(
        SegmentProfile::Segmented16,
    ));
    // Destination fetch belongs to the next entry, even when restored NT is set.
    image.check_unchanged_exit(
        step,
        "destination fetch precedes checking restored NT",
        Exit::PageFault {
            address: 0xc200,
            error: 0x10,
        },
    );
    image.map(0xc, 0x5000, false);
    image.data(0x5200, &[0xcf]);
    image.check_unchanged_exit(
        step,
        "restored NT applies at the next IRET",
        Exit::Other(0x0008_00cf_0000_0200),
    );
}

#[test]
fn nt_defined_by_popf_in_the_same_block_selects_the_task_return_path() {
    // POPFD loads NT; IRET must observe that definition before any stack access.
    let code = [0x9d, 0xcf];
    let profile = ExecutionProfile::Protected(SegmentProfile::Flat32);
    let mut image = image(&code, profile);
    image.cpu.registers.esp = 0x4ffc;
    image.map(4, 0xa000, false);
    image.data(0xaffc, &[0, 0x40, 0, 0]);
    let mut cpu = cleared_flags(image.cpu, false);
    cpu.flags.bytes.nt = 1;
    cpu.registers.esp = 0x5000;
    cpu.eip = 0x1001;
    cpu.instruction_count = 0;
    let unsupported = || Step {
        cpu,
        ram: &[],
        exit: Exit::Other(0x0008_00cf_0000_1001),
    };
    assert_eq!(
        TestModule::interpreter().observe(&image.input(), 2),
        expected(
            &image,
            &[
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip)
                },
                unsupported(),
            ]
        )
    );
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 2, profile);
    assert_eq!(
        block.observe(&image.input(), 1),
        expected(&image, &[unsupported()])
    );
}

#[test]
fn complete_fetch_precedes_nt_and_byte_sixteen_is_rejected() {
    for (length, suffix) in [(1, 0xcf), (15, 0xcf), (16, 0xcf)] {
        let code = [vec![0x67; length - 1], vec![suffix]].concat();
        let mut image = image(&[], ExecutionProfile::Protected(SegmentProfile::Flat32));
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code[..length.min(15)]);
        image.cpu.flags.bytes.nt = 1;
        let exit = if length <= 15 {
            Exit::Other(0x0008_00cf_0000_1ff1)
        } else {
            Exit::GeneralProtection { error: 0 }
        };
        if length <= 15 {
            check_one(
                ExecutionProfile::Protected(SegmentProfile::Flat32),
                &code,
                &image,
                &[],
                Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit,
                },
            );
        } else {
            assert!(matches!(
                crate::compile_block_from_bytes(0x1ff1, &code, 1),
                Err(crate::BlockError::InstructionTooLong { address: 0x1ff1 })
            ));
            image.check_unchanged_exit(
                TestModule::interpreter(),
                "IRET exceeds fifteen bytes before checking NT",
                exit,
            );
        }
    }
    let mut image = image(&[], ExecutionProfile::Protected(SegmentProfile::Flat32));
    image.cpu.eip = 0x1fff;
    image.cpu.flags.bytes.nt = 1;
    image.data(0x3fff, &[0x66]);
    image.check_unchanged_exit(
        TestModule::interpreter(),
        "IRET prefix fetch precedes checking NT",
        Exit::PageFault {
            address: 0x2000,
            error: 0x10,
        },
    );
}
