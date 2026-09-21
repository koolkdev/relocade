use super::stack_segment;
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{Gpr32::*, Segment};

#[rustfmt::skip]
fn widths() -> Vec<Case> {
    [
        (true, false, 0x1234_fffc, 0x1235_0000, 0x89ab_cdef),
        (true, true, 0x1234_fffc, 0x1234_fffe, 0x1234_cdef),
        (false, false, 0xfffc, 0xabcd_0000, 0x89ab_cdef),
        (false, true, 0xfffc, 0xabcd_fffe, 0x1234_cdef),
    ].into_iter().map(|(big, word, offset, next_esp, next_ebp)| {
        let code = if word { vec![0x66, 0xc9] } else { vec![0xc9] };
        Case::preserving_flags(format!("LEAVE stack32={big} operand16={word}"), &code)
            .segmented_only().segment(Segment::Ss, stack_segment(0x20000, u32::MAX, big))
            .register(Esp, 0xabcd_3000, next_esp).register(Ebp, 0x1234_fffc, next_ebp)
            .memory(0x20000 + offset, &[0xef, 0xcd, 0xab, 0x89][..if word { 2 } else { 4 }], ReadOnly)
    }).collect()
}

#[rustfmt::skip]
fn frame_boundaries() -> Vec<Case> {
    vec![
        Case::preserving_flags("word LEAVE reads exactly the last two bytes of a page", &[0x66, 0xc9])
            .register(Esp, 0xdead_beef, 0x5000).register(Ebp, 0x4ffe, 0xcdef).memory(0x4ffe, &[0xef, 0xcd], ReadOnly),
        Case::preserving_flags("LEAVE discards an out-of-limit entry stack pointer", &[0xc9])
            .segmented_only().segment(Segment::Ss, stack_segment(0x20000, 0x8003, true))
            .register(Esp, 0xffff_ffff, 0x8004).register(Ebp, 0x8000, 0x89ab_cdef)
            .memory(0x28000, &[0xef, 0xcd, 0xab, 0x89], ReadOnly),
        Case::preserving_flags("LEAVE reads a complete straddling frame then wraps SP", &[0xc9])
            .segmented_only().segment(Segment::Ss, stack_segment(0, 0x10001, false))
            .register(Esp, 0xabcd_3000, 0xabcd_0002).register(Ebp, 0xbeef_fffe, 0x89ab_cdef)
            .memory(0xfffe, &[0xef, 0xcd, 0xab, 0x89], ReadOnly),
    ]
}

#[rustfmt::skip]
fn faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("LEAVE keeps both entry pointers on a missing frame", &[0xc9])
            .initial_registers(&[(Esp, 0x9000), (Ebp, 0x5000)]).fault(0x5000, 0),
        Case::preserving_flags("word LEAVE keeps both entry pointers on an incomplete frame", &[0x66, 0xc9])
            .initial_registers(&[(Esp, 0xabcd_9000), (Ebp, 0x1234_4fff)])
            .memory(0x1234_4fff, &[0xef], ReadOnly).fault(0x1234_5000, 0),
        Case::preserving_flags("LEAVE checks the frame SS span before paging or committing either pointer", &[0xc9])
            .segmented_only().segment(Segment::Ss, stack_segment(0x20000, 0x4fff, false))
            .initial_registers(&[(Esp, 0xabcd_9000), (Ebp, 0x1234_4fff)]).stack_fault(0),
    ]
}

#[rustfmt::skip]
fn procedure_frames() -> Vec<Sequence> {
    vec![
        Sequence::preserving_flags("LEAVE consumes the frame built earlier in the block and RET uses its stack")
            .initial_registers(&[(Esp, 0x9000), (Ebp, 0xbeef_5678)])
            .memory(0x8ffc, &[0xff, 0xff, 0xff, 0xff, 0, 0x20, 0, 0], ReadWrite)
            .step(Step::preserving_flags(&[0x55]).register(Esp, 0x8ffc).expect_memory(0x8ffc, &[0x78, 0x56, 0xef, 0xbe]))
            .step(Step::preserving_flags(&[0x89, 0xe5]).register(Ebp, 0x8ffc))
            .step(Step::preserving_flags(&[0xc9]).register(Esp, 0x9000).register(Ebp, 0xbeef_5678))
            .step(Step::preserving_flags(&[0xc3]).register(Esp, 0x9004).dispatch(0x2000)),
        Sequence::preserving_flags("word LEAVE observes a prior BP write and feeds subsequent EBP and ESP reads")
            .initial_registers(&[(Esp, 0x9000), (Ebp, 0x1234_8000)]).memory(0x1234_4ffe, &[0xef, 0xcd], ReadOnly)
            .step(Step::preserving_flags(&[0x66, 0xbd, 0xfe, 0x4f]).register(Ebp, 0x1234_4ffe))
            .step(Step::preserving_flags(&[0x66, 0xc9]).register(Esp, 0x1234_5000).register(Ebp, 0x1234_cdef))
            .step(Step::preserving_flags(&[0x89, 0xe8]).register(Eax, 0x1234_cdef))
            .step(Step::preserving_flags(&[0x89, 0xe7]).register(Edi, 0x1234_5000)),
        Sequence::from_opaque_flags("faulting LEAVE preserves preceding pointer writes and arithmetic")
            .initial_registers(&[(Eax, 0x7fff_ffff), (Ebp, 0x6000), (Esp, 0x9000)])
            .memory(0x4fff, &[0xef], ReadOnly)
            .step(Step::new(&[0x83, 0xc0, 1], Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x8000_0000))
            .step(Step::preserving_flags(&[0xbd, 0xff, 0x4f, 0, 0]).register(Ebp, 0x4fff))
            .step(Step::preserving_flags(&[0x66, 0xc9]).fault(0x5000, 0))
            .trailing_code(&[0x89, 0xc7], 1),
    ]
}

test_cases!(independent_operand_and_stack_widths, widths());
test_cases!(stack_frame_boundaries, frame_boundaries());
test_cases!(faults_preserve_both_entry_pointers, faults());
test_sequences!(frame_restoration_and_fault_progress, procedure_frames());
