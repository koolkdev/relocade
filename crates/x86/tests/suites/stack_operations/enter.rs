use super::{code16, stack_segment};
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

#[rustfmt::skip]
fn widths() -> Vec<Case> {
    let mut cases = Vec::new();
    for (big, word, destination, source, next_esp, next_ebp, frame) in [
        (true, false, 0x1234_8ff4, 0xabcd_7ffc, 0x1234_8fd4, 0x1234_8ffc,
            &[0xfc, 0x8f, 0x34, 0x12, 0xef, 0xcd, 0xab, 0x89, 0, 0x80, 0xcd, 0xab][..]),
        (true, true, 0x1234_8ffa, 0xabcd_7ffe, 0x1234_8fda, 0xabcd_8ffe, &[0xfe, 0x8f, 0xef, 0xcd, 0, 0x80][..]),
        (false, false, 0x8ff4, 0x7ffc, 0x1234_8fd4, 0x1234_8ffc,
            &[0xfc, 0x8f, 0x34, 0x12, 0xef, 0xcd, 0xab, 0x89, 0, 0x80, 0xcd, 0xab][..]),
        (false, true, 0x8ffa, 0x7ffe, 0x1234_8fda, 0xabcd_8ffe, &[0xfe, 0x8f, 0xef, 0xcd, 0, 0x80][..]),
    ] {
        let code = if word { vec![0x66, 0xc8, 0x20, 0, 2] } else { vec![0xc8, 0x20, 0, 2] };
        cases.push(Case::preserving_flags(format!("ENTER stack32={big} operand16={word}"), &code)
            .segmented_only().segment(Segment::Ss, stack_segment(0x20000, u32::MAX, big))
            .register(Esp, 0x1234_9000, next_esp).register(Ebp, 0xabcd_8000, next_ebp)
            .memory(0x20000 + destination, &vec![0xa5; frame.len()], ReadWrite)
            .memory(0x20000 + source, &[0xef, 0xcd, 0xab, 0x89][..if word { 2 } else { 4 }], ReadOnly)
            .expect_memory(0x20000 + destination, frame));
    }
    cases
}

fn nesting_levels() -> Vec<Case> {
    let mut cases = Vec::new();
    for (level, next_esp, frame) in [
        (0, 0x8ffcu32, &[0, 0x70, 0x34, 0x12][..]),
        (1, 0x8ff8, &[0xfc, 0x8f, 0, 0, 0, 0x70, 0x34, 0x12][..]),
        (
            2,
            0x8ff4,
            &[
                0xfc, 0x8f, 0, 0, 0x44, 0x33, 0x22, 0x11, 0, 0x70, 0x34, 0x12,
            ][..],
        ),
        (32, 0x8ffc, &[0, 0x70, 0x34, 0x12][..]),
        (33, 0x8ff8, &[0xfc, 0x8f, 0, 0, 0, 0x70, 0x34, 0x12][..]),
    ] {
        let mut case =
            Case::preserving_flags(format!("ENTER nesting byte {level}"), &[0xc8, 0, 0, level])
                .register(Esp, 0x9000, next_esp)
                .register(Ebp, 0x1234_7000, 0x8ffc)
                .memory(next_esp, &vec![0xa5; frame.len()], ReadWrite)
                .expect_memory(next_esp, frame);
        if level == 2 {
            case = case.memory(0x1234_6ffc, &[0x44, 0x33, 0x22, 0x11], ReadOnly);
        }
        cases.push(case);
    }
    for (level, prefix, next_esp, next_ebp, frame) in [
        (
            31,
            &[][..],
            0x8f80,
            0x8ffc,
            [vec![0xfc, 0x8f, 0, 0], vec![0x5a; 120], vec![0, 0x70, 0, 0]].concat(),
        ),
        (
            255,
            &[0x66][..],
            0x8fc0,
            0x8ffe,
            [vec![0xfe, 0x8f], vec![0x5a; 60], vec![0, 0x70]].concat(),
        ),
    ] {
        let code = [prefix, &[0xc8, 0, 0, level]].concat();
        cases.push(
            Case::preserving_flags(format!("maximum ENTER nesting {code:02x?}"), &code)
                .register(Esp, 0x9000, next_esp)
                .register(Ebp, 0x7000, next_ebp)
                .memory(0x6f88, &[0x5a; 120], ReadOnly)
                .memory(next_esp, &vec![0xa5; frame.len()], ReadWrite)
                .expect_memory(next_esp, &frame),
        );
    }
    cases
}

#[rustfmt::skip]
fn frame_boundaries() -> Vec<Case> {
    vec![
        Case::preserving_flags("word ENTER allocation borrows across 64K on a big stack", &[0x66, 0xc8, 0x20, 0, 0])
            .register(Esp, 0x1234_0002, 0x1233_ffe0).register(Ebp, 0xabcd_8000, 0xabcd_0000)
            .memory(0x1234_0000, &[0xa5; 2], ReadWrite).memory(0x1233_ffe0, &[0x5a; 2], ReadWrite)
            .expect_memory(0x1234_0000, &[0, 0x80]),
        Case::preserving_flags("word ENTER source borrow must not change final EBP upper half", &[0x66, 0xc8, 0, 0, 2])
            .register(Esp, 0x9000, 0x8ffa).register(Ebp, 0x1234_0000, 0x1234_8ffe)
            .memory(0x1233_fffe, &[0x34, 0x12], ReadOnly).memory(0x8ffa, &[0xa5; 6], ReadWrite)
            .expect_memory(0x8ffa, &[0xfe, 0x8f, 0x34, 0x12, 0, 0]),
        Case::preserving_flags("dword ENTER wraps each small-stack push and allocation separately", &[0xc8, 0x10, 0, 2])
            .segmented_only().segment(Segment::Ss, stack_segment(0, 0xffff, false))
            .register(Esp, 0xabcd_0004, 0xabcd_ffe8).register(Ebp, 0x1234_0000, 0xabcd_0000)
            .memory(0, &[0xa5; 4], ReadWrite)
            .memory(0xfff8, &[0x44, 0x33, 0x22, 0x11, 0x78, 0x56, 0x34, 0x12], ReadWrite)
            .expect_memory(0, &[0, 0, 0x34, 0x12]).expect_memory(0xfff8, &[0, 0, 0xcd, 0xab, 0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags("ENTER display reads observe an earlier overlapping push", &[0xc8, 0, 0, 3])
            .register(Esp, 0x9000, 0x8ff0).register(Ebp, 0x9000, 0x8ffc).memory(0x8ff0, &[0xa5; 16], ReadWrite)
            .expect_memory(0x8ff0, &[0xfc, 0x8f, 0, 0, 0, 0x90, 0, 0, 0, 0x90, 0, 0, 0, 0x90, 0, 0]),
        Case::preserving_flags("ENTER allocates an unsigned size without accessing intermediate pages", &[0xc8, 0xff, 0xff, 0])
            .register(Esp, 0x19004, 0x9001).register(Ebp, 0xbeef_5678, 0x19000).memory(0x19000, &[0xa5; 4], ReadWrite)
            .memory(0x9001, &[0x5a; 4], ReadWrite).expect_memory(0x19000, &[0x78, 0x56, 0xef, 0xbe]),
        Case::preserving_flags("ENTER display reads observe aliased physical stack pages", &[0xc8, 0, 0, 3])
            .register(Esp, 0x9000, 0x8ff0).register(Ebp, 0xa000, 0x8ffc).map_page(8, 0x8000, ReadWrite)
            .map_page(9, 0x8000, ReadOnly).memory(0x8ff0, &[0xa5; 16], ReadWrite)
            .expect_memory(0x8ff0, &[0xfc, 0x8f, 0, 0, 0, 0xa0, 0, 0, 0, 0xa0, 0, 0, 0, 0xa0, 0, 0]),
    ]
}

#[rustfmt::skip]
fn fault_stages() -> Vec<Case> {
    vec![
        Case::preserving_flags("ENTER first split push preserves both pointers and all bytes", &[0x66, 0xc8, 0, 0, 0])
            .initial_registers(&[(Esp, 0x5001), (Ebp, 0x1234_5678)])
            .memory(0x4fff, &[0xa5], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("ENTER retains copied frames on a later display read fault", &[0xc8, 0, 0, 3])
            .initial_register(Esp, 0x9000).initial_register(Ebp, 0x5004)
            .memory(0x5000, &[0x78, 0x56, 0x34, 0x12], ReadOnly).memory(0x8ff0, &[0xa5; 16], ReadWrite)
            .expect_memory(0x8ff8, &[0x78, 0x56, 0x34, 0x12, 4, 0x50, 0, 0]).fault(0x4ffc, 0),
        Case::preserving_flags("ENTER retains the saved pointer when a copied-frame push faults", &[0xc8, 0, 0, 2])
            .initial_register(Esp, 0x5004).initial_register(Ebp, 0x9004).memory(0x5000, &[0xa5; 4], ReadWrite)
            .memory(0x9000, &[0x78, 0x56, 0x34, 0x12], ReadOnly).expect_memory(0x5000, &[4, 0x90, 0, 0])
            .fault(0x4ffc, 2),
        Case::preserving_flags("ENTER self-link push may fault after the saved pointer", &[0xc8, 0, 0, 1])
            .initial_register(Esp, 0x5004).initial_register(Ebp, 0x9004).memory(0x5000, &[0xa5; 4], ReadWrite)
            .expect_memory(0x5000, &[4, 0x90, 0, 0]).fault(0x4ffc, 2),
        Case::preserving_flags("word ENTER retains only completed pushes on a split display read fault", &[0x66, 0xc8, 0, 0, 2])
            .initial_register(Esp, 0x9000).initial_register(Ebp, 0x5001).memory(0x8ffa, &[0xa5; 6], ReadWrite)
            .memory(0x4fff, &[0x34], ReadOnly).expect_memory(0x8ffe, &[1, 0x50]).fault(0x5000, 0),
        Case::preserving_flags("ENTER final write probe checks the complete dword without storing it", &[0xc8, 1, 0x10, 0])
            .initial_register(Esp, 0x9004).initial_register(Ebp, 0x1234_5678).memory(0x9000, &[0xa5; 4], ReadWrite)
            .memory(0x7fff, &[0x5a], ReadWrite).expect_memory(0x9000, &[0x78, 0x56, 0x34, 0x12]).fault(0x8000, 2),
        Case::preserving_flags("ENTER final write probe checks the complete word without storing it", &[0x66, 0xc8, 1, 0x10, 0])
            .initial_register(Esp, 0x9002).initial_register(Ebp, 0x1234_5678).memory(0x9000, &[0xa5; 2], ReadWrite)
            .memory(0x7fff, &[0x5a], ReadWrite).expect_memory(0x9000, &[0x78, 0x56]).fault(0x8000, 2),
        Case::preserving_flags("ENTER allocation probe requires write permission after saving BP", &[0x66, 0xc8, 4, 0x10, 0])
            .initial_registers(&[(Esp, 0x9002), (Ebp, 0x1234_5678)])
            .memory(0x9000, &[0xa5; 2], ReadWrite).expect_memory(0x9000, &[0x78, 0x56])
            .memory(0x7ffc, &[0x5a; 2], ReadOnly).fault(0x7ffc, 3),
        Case::preserving_flags("ENTER first push checks the whole SS span before paging", &[0xc8, 0, 0, 0])
            .segmented_only().segment(Segment::Ss, stack_segment(0x20000, 0x4fff, true))
            .initial_registers(&[(Esp, 0x5002), (Ebp, 0x7000)]).stack_fault(0),
        Case::preserving_flags("ENTER display SS fault retains its first push and entry pointers", &[0xc8, 0, 0, 2])
            .segmented_only().segment(Segment::Ss, stack_segment(0x20000, 0x4fff, false))
            .initial_registers(&[(Esp, 0x4004), (Ebp, 0x6000)])
            .memory(0x24000, &[0xa5; 4], ReadWrite).expect_memory(0x24000, &[0, 0x60, 0, 0]).stack_fault(0),
        Case::preserving_flags("ENTER allocation below an expand-down limit faults before paging", &[0xc8, 1, 0x20, 0])
            .segmented_only().segment(Segment::Ss, StoredSegment {
                attributes: SegmentAttributes::from_bits(0x1d), ..stack_segment(0x20000, 0x7fff, true)
            })
            .initial_registers(&[(Esp, 0x9004), (Ebp, 0x1234_5678)])
            .memory(0x29000, &[0xa5; 4], ReadWrite).expect_memory(0x29000, &[0x78, 0x56, 0x34, 0x12])
            .stack_fault(0),
    ]
}

#[rustfmt::skip]
fn procedure_frames() -> Vec<Sequence> {
    vec![
        Sequence::preserving_flags("ENTER frame feeds LEAVE and RET in the same block")
            .initial_registers(&[(Esp, 0x9000), (Ebp, 0xbeef_5678)])
            .memory(0x8ff8, &[0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0, 0x20, 0, 0], ReadWrite)
            .step(Step::preserving_flags(&[0xc8, 0x20, 0, 1])
                .register(Esp, 0x8fd8).register(Ebp, 0x8ffc).expect_memory(0x8ff8, &[0xfc, 0x8f, 0, 0, 0x78, 0x56, 0xef, 0xbe]))
            .step(Step::preserving_flags(&[0xc9]).register(Esp, 0x9000).register(Ebp, 0xbeef_5678))
            .step(Step::preserving_flags(&[0xc3]).register(Esp, 0x9004).dispatch(0x2000)),
        Sequence::preserving_flags("word procedure frames preserve independent pointer upper halves")
            .segmented_only().segment(Segment::Cs, code16()).segment(Segment::Ss, stack_segment(0, 0xffff, false))
            .initial_registers(&[(Esp, 0xabcd_9000), (Ebp, 0xbeef_5678)])
            .memory(0x8ffe, &[0xa5, 0xa5, 0, 0x20], ReadWrite)
            .step(Step::preserving_flags(&[0xc8, 0x20, 0, 0])
                .register(Esp, 0xabcd_8fde).register(Ebp, 0xbeef_8ffe).expect_memory(0x8ffe, &[0x78, 0x56]))
            .step(Step::preserving_flags(&[0xc9]).register(Esp, 0xabcd_9000).register(Ebp, 0xbeef_5678))
            .step(Step::preserving_flags(&[0xc3]).register(Esp, 0xabcd_9002).dispatch(0x2000)),
        Sequence::preserving_flags("ENTER observes a prior BP write and feeds full EBP and ESP consumers")
            .initial_registers(&[(Esp, 0x9000), (Ebp, 0x1234_7000)]).memory(0x8ffe, &[0xa5; 2], ReadWrite)
            .step(Step::preserving_flags(&[0x66, 0xbd, 0x78, 0x56]).register(Ebp, 0x1234_5678))
            .step(Step::preserving_flags(&[0x66, 0xc8, 0x20, 0, 0])
                .register(Esp, 0x8fde).register(Ebp, 0x1234_8ffe).expect_memory(0x8ffe, &[0x78, 0x56]))
            .step(Step::preserving_flags(&[0x89, 0xe8]).register(Eax, 0x1234_8ffe))
            .step(Step::preserving_flags(&[0x89, 0xe1]).register(Ecx, 0x8fde)),
        Sequence::from_opaque_flags("ENTER display fault publishes preceding arithmetic and pointer writes")
            .initial_registers(&[(Eax, 0x7fff_ffff), (Esp, 0x9000), (Ebp, 0x6000)])
            .memory(0x8ff4, &[0xa5; 12], ReadWrite)
            .step(Step::new(&[0x83, 0xc0, 1], Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x8000_0000))
            .step(Step::preserving_flags(&[0xbd, 0, 0x70, 0, 0]).register(Ebp, 0x7000))
            .step(Step::preserving_flags(&[0xc8, 0, 0, 2]).expect_memory(0x8ffc, &[0, 0x70, 0, 0]).fault(0x6ffc, 0))
            .trailing_code(&[0x89, 0xc7], 1),
    ]
}

test_cases!(independent_operand_and_stack_widths, widths());
test_cases!(nesting_levels_and_masked_immediate, nesting_levels());
test_cases!(stack_boundaries_and_overlapping_frames, frame_boundaries());
test_cases!(faults_retain_only_completed_pushes, fault_stages());
test_sequences!(procedure_frames_and_fault_progress, procedure_frames());
