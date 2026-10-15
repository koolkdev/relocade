use crate::support::{
    cases::{test_cases, InstructionCase as Case, Permissions::ReadWrite},
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
};
use crate::{ExecutionProfile, Gpr32::*, Segment, SegmentAttributes, Segments, StoredSegment};

fn protected_forms() -> Vec<Case> {
    let mut cases = vec![
        Case::preserving_flags("SMSW EAX clears undefined high bits", &[0x0f, 0x01, 0xe0])
            .register(Eax, 0xabcd_ffff, 0x33),
        Case::preserving_flags(
            "SMSW AX preserves the upper half",
            &[0x66, 0x0f, 0x01, 0xe0],
        )
        .register(Eax, 0xabcd_ffff, 0xabcd_0033),
        Case::preserving_flags("SMSW memory is a word at the page end", &[0x0f, 0x01, 0x23])
            .initial_register(Ebx, 0x5ffe)
            .memory(0x5ffd, &[0xaa, 0xbb, 0xcc], ReadWrite)
            .expect_memory(0x5ffe, &[0x33, 0]),
        Case::preserving_flags(
            "SMSW checks both bytes before a split store",
            &[0x0f, 0x01, 0x23],
        )
        .initial_register(Ebx, 0x5fff)
        .memory(0x5fff, &[0xaa], ReadWrite)
        .fault(0x6000, 2),
        Case::preserving_flags(
            "SMSW checks the segment before the unmapped destination",
            &[0x65, 0x0f, 0x01, 0x23],
        )
        .initial_register(Ebx, 0xffff)
        .segment(
            Segment::Gs,
            StoredSegment {
                limit: 0xffff,
                ..StoredSegment::flat_data32(0x2b)
            },
        )
        .general_protection(0),
    ];
    for (code, output) in [
        (&[0x0f, 0x01, 0xe0][..], 0xabcd_0033),
        (&[0x66, 0x0f, 0x01, 0xe0][..], 0x33),
    ] {
        cases.push(
            Case::preserving_flags("SMSW follows a 16-bit code default", code)
                .segmented_only()
                .segment(
                    Segment::Cs,
                    StoredSegment {
                        attributes: SegmentAttributes::from_bits(7),
                        ..StoredSegment::flat_code32(0x1b)
                    },
                )
                .register(Eax, 0xabcd_ffff, output),
        );
    }
    cases
}
test_cases!(protected_operand_forms, protected_forms());

fn real_forms(engine: Engine, frontend: Frontend) {
    let mut cases = ImageSequences::new(engine, frontend, ExecutionProfile::Real16);
    for (code, eax) in [
        (&[0x0f, 0x01, 0xe0][..], 0xabcd_0032),
        (&[0x66, 0x0f, 0x01, 0xe0][..], 0x32),
    ] {
        let mut image = Image::new(code);
        image.cpu.segments = Segments::real_mode();
        image.cpu.registers.eax = 0xabcd_ffff;
        let mut cpu = image.cpu;
        cpu.registers.eax = eax;
        cpu.eip += code.len() as u32;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        cases.check(
            "real-mode SMSW clears PE",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
    for code in [
        &[0x0f, 0x01, 0x26, 0xfe, 0xff][..],
        &[0x66, 0x0f, 0x01, 0x26, 0xfe, 0xff][..],
    ] {
        let mut image = Image::new(code);
        image.cpu.segments = Segments::real_mode();
        image.map(0xf, 0x8000, true);
        image.data(0x8ffd, &[0xaa, 0xbb, 0xcc]);
        let mut cpu = image.cpu;
        cpu.eip += code.len() as u32;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
        cases.check(
            "real-mode SMSW always stores two bytes",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[(0x8ffe, &[0x32, 0])],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}
test_frontends!(real_operand_forms, real_forms);
