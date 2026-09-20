use super::{code16, stack_segment};
use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
        Permissions::ReadWrite,
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::{Gpr32::*, Segment};

fn procedure_frames() -> Vec<Case> {
    vec![
        Case::preserving_flags("ENTER frame feeds LEAVE and RET in the same block")
            .initial_registers(&[(Esp, 0x9000), (Ebp, 0xbeef_5678)])
            .memory(
                0x8ff8,
                &[
                    0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0, 0x20, 0, 0,
                ],
                ReadWrite,
            )
            .step(
                Step::preserving_flags(&[0xc8, 0x20, 0, 1])
                    .register(Esp, 0x8fd8)
                    .register(Ebp, 0x8ffc)
                    .expect_memory(0x8ff8, &[0xfc, 0x8f, 0, 0, 0x78, 0x56, 0xef, 0xbe]),
            )
            .step(
                Step::preserving_flags(&[0xc9])
                    .register(Esp, 0x9000)
                    .register(Ebp, 0xbeef_5678),
            )
            .step(
                Step::preserving_flags(&[0xc3])
                    .register(Esp, 0x9004)
                    .dispatch(0x2000),
            ),
        Case::preserving_flags("word procedure frames preserve independent pointer upper halves")
            .segmented_only()
            .segment(Segment::Cs, code16())
            .segment(Segment::Ss, stack_segment(0, 0xffff, false))
            .initial_registers(&[(Esp, 0xabcd_9000), (Ebp, 0xbeef_5678)])
            .memory(0x8ffe, &[0xa5, 0xa5, 0, 0x20], ReadWrite)
            .step(
                Step::preserving_flags(&[0xc8, 0x20, 0, 0])
                    .register(Esp, 0xabcd_8fde)
                    .register(Ebp, 0xbeef_8ffe)
                    .expect_memory(0x8ffe, &[0x78, 0x56]),
            )
            .step(
                Step::preserving_flags(&[0xc9])
                    .register(Esp, 0xabcd_9000)
                    .register(Ebp, 0xbeef_5678),
            )
            .step(
                Step::preserving_flags(&[0xc3])
                    .register(Esp, 0xabcd_9002)
                    .dispatch(0x2000),
            ),
        Case::preserving_flags(
            "ENTER observes a prior BP write and feeds full EBP and ESP consumers",
        )
        .initial_registers(&[(Esp, 0x9000), (Ebp, 0x1234_7000)])
        .memory(0x8ffe, &[0xa5; 2], ReadWrite)
        .step(Step::preserving_flags(&[0x66, 0xbd, 0x78, 0x56]).register(Ebp, 0x1234_5678))
        .step(
            Step::preserving_flags(&[0x66, 0xc8, 0x20, 0, 0])
                .register(Esp, 0x8fde)
                .register(Ebp, 0x1234_8ffe)
                .expect_memory(0x8ffe, &[0x78, 0x56]),
        )
        .step(Step::preserving_flags(&[0x89, 0xe8]).register(Eax, 0x1234_8ffe))
        .step(Step::preserving_flags(&[0x89, 0xe1]).register(Ecx, 0x8fde)),
        Case::preserving_flags("completed ENTER survives a later memory fault")
            .initial_registers(&[(Esp, 0x9000), (Ebp, 0x1234_7000), (Eax, 0x6000)])
            .memory(0x8ffc, &[0xa5; 4], ReadWrite)
            .step(
                Step::preserving_flags(&[0xc8, 0x20, 0, 0])
                    .register(Esp, 0x8fdc)
                    .register(Ebp, 0x8ffc)
                    .expect_memory(0x8ffc, &[0, 0x70, 0x34, 0x12]),
            )
            .step(Step::preserving_flags(&[0x8b, 0x00]).fault(0x6000, 0))
            .trailing_code(&[0x89, 0xe7], 1),
        Case::from_opaque_flags(
            "ENTER display fault publishes preceding arithmetic and pointer writes",
        )
        .initial_registers(&[(Eax, 0x7fff_ffff), (Esp, 0x9000), (Ebp, 0x6000)])
        .memory(0x8ff4, &[0xa5; 12], ReadWrite)
        .step(
            Step::new(
                &[0x83, 0xc0, 1],
                Flags {
                    cf: Clear,
                    pf: Set,
                    af: Set,
                    zf: Clear,
                    sf: Set,
                    of: Set,
                },
            )
            .register(Eax, 0x8000_0000),
        )
        .step(Step::preserving_flags(&[0xbd, 0, 0x70, 0, 0]).register(Ebp, 0x7000))
        .step(
            Step::preserving_flags(&[0xc8, 0, 0, 2])
                .expect_memory(0x8ffc, &[0, 0x70, 0, 0])
                .fault(0x6ffc, 0),
        )
        .trailing_code(&[0x89, 0xc7], 1),
    ]
}

test_sequences!(procedure_frames_and_fault_progress, procedure_frames());
