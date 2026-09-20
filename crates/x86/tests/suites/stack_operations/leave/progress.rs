use super::{code16, stack_segment};
use crate::support::{
    cases::{
        FlagExpectation::{Clear, Set},
        Flags,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::{Gpr32::*, Segment};

fn procedure_frames() -> Vec<Case> {
    vec![
        Case::preserving_flags(
            "LEAVE consumes the frame built earlier in the block and RET uses its stack",
        )
        .initial_registers(&[(Esp, 0x9000), (Ebp, 0xbeef_5678)])
        .memory(0x8ffc, &[0xff, 0xff, 0xff, 0xff, 0, 0x20, 0, 0], ReadWrite)
        .step(
            Step::preserving_flags(&[0x55])
                .register(Esp, 0x8ffc)
                .expect_memory(0x8ffc, &[0x78, 0x56, 0xef, 0xbe]),
        )
        .step(Step::preserving_flags(&[0x89, 0xe5]).register(Ebp, 0x8ffc))
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
        Case::preserving_flags(
            "word LEAVE and RET preserve the independent high halves on a 16-bit stack",
        )
        .segmented_only()
        .segment(Segment::Cs, code16())
        .segment(Segment::Ss, stack_segment(0, 0xffff, false))
        .initial_registers(&[(Esp, 0xabcd_9000), (Ebp, 0xbeef_5678)])
        .memory(0x8ffe, &[0xff, 0xff, 0, 0x20], ReadWrite)
        .step(
            Step::preserving_flags(&[0x55])
                .register(Esp, 0xabcd_8ffe)
                .expect_memory(0x8ffe, &[0x78, 0x56]),
        )
        .step(Step::preserving_flags(&[0x89, 0xe5]).register(Ebp, 0xbeef_8ffe))
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
            "word LEAVE observes a prior BP write and feeds subsequent EBP and ESP reads",
        )
        .initial_registers(&[(Esp, 0x9000), (Ebp, 0x1234_8000)])
        .memory(0x1234_4ffe, &[0xef, 0xcd], ReadOnly)
        .step(Step::preserving_flags(&[0x66, 0xbd, 0xfe, 0x4f]).register(Ebp, 0x1234_4ffe))
        .step(
            Step::preserving_flags(&[0x66, 0xc9])
                .register(Esp, 0x1234_5000)
                .register(Ebp, 0x1234_cdef),
        )
        .step(Step::preserving_flags(&[0x89, 0xe8]).register(Eax, 0x1234_cdef))
        .step(Step::preserving_flags(&[0x89, 0xe7]).register(Edi, 0x1234_5000)),
        Case::preserving_flags("completed LEAVE survives a later operand fault")
            .initial_registers(&[(Esp, 0x9000), (Ebp, 0x8000), (Eax, 0x6000)])
            .memory(0x8000, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
            .step(
                Step::preserving_flags(&[0xc9])
                    .register(Esp, 0x8004)
                    .register(Ebp, 0x1234_5678),
            )
            .step(Step::preserving_flags(&[0x8b, 0x00]).fault(0x6000, 0))
            .trailing_code(&[0x89, 0xe7], 1),
    ]
}

fn prior_progress() -> Vec<Case> {
    [false, true].into_iter().map(|segment_fault| {
        let fault = if segment_fault {
            Step::preserving_flags(&[0x66, 0xc9]).stack_fault(0)
        } else {
            Step::preserving_flags(&[0x66, 0xc9]).fault(0x15000, 0)
        };
        Case::from_opaque_flags(format!("faulting LEAVE preserves earlier register and arithmetic effects; SS fault={segment_fault}"))
            .segmented_only()
            .segment(Segment::Ss, stack_segment(0x10000, if segment_fault { 0x4fff } else { 0xffff }, true))
            .initial_registers(&[(Eax, 0x7fff_ffff), (Ebp, 0x6000), (Esp, 0x9000)])
            .memory(0x14fff, &[0xef], ReadOnly)
            .step(Step::new(&[0x83, 0xc0, 1], Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Eax, 0x8000_0000))
            .step(Step::preserving_flags(&[0xbd, 0xff, 0x4f, 0, 0]).register(Ebp, 0x4fff))
            .step(fault)
            .trailing_code(&[0x89, 0xc7], 1)
    }).collect()
}

test_sequences!(
    frame_restoration_and_following_instructions,
    procedure_frames()
);
test_sequences!(faulting_leave_publishes_only_prior_work, prior_progress());
