use super::{code16, stack_segment};
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    Gpr32::{Ebp, Esp},
    Segment, SegmentAttributes, StoredSegment,
};

#[path = "enter/faults.rs"]
mod faults;
#[path = "enter/progress.rs"]
mod progress;

fn widths() -> Vec<Case> {
    let mut cases = Vec::new();
    for (big, word, destination, source, next_esp, next_ebp, frame) in [
        (
            true,
            false,
            0x1234_8ff4u32,
            0xabcd_7ffcu32,
            0x1234_8fd4,
            0x1234_8ffc,
            &[
                0xfc, 0x8f, 0x34, 0x12, 0xef, 0xcd, 0xab, 0x89, 0, 0x80, 0xcd, 0xab,
            ][..],
        ),
        (
            true,
            true,
            0x1234_8ffa,
            0xabcd_7ffe,
            0x1234_8fda,
            0xabcd_8ffe,
            &[0xfe, 0x8f, 0xef, 0xcd, 0, 0x80][..],
        ),
        (
            false,
            false,
            0x8ff4,
            0x7ffc,
            0x1234_8fd4,
            0x1234_8ffc,
            &[
                0xfc, 0x8f, 0x34, 0x12, 0xef, 0xcd, 0xab, 0x89, 0, 0x80, 0xcd, 0xab,
            ][..],
        ),
        (
            false,
            true,
            0x8ffa,
            0x7ffe,
            0x1234_8fda,
            0xabcd_8ffe,
            &[0xfe, 0x8f, 0xef, 0xcd, 0, 0x80][..],
        ),
    ] {
        for default_word in [false, true] {
            for ignored in [&[][..], &[0x64, 0x67]] {
                for base in [0, 0x20000] {
                    let mut code = ignored.to_vec();
                    if word != default_word {
                        code.push(0x66);
                    }
                    code.extend([0xc8, 0x20, 0, 2]);
                    let mut case = Case::preserving_flags(
                        format!("ENTER B={big} word={word} code16={default_word} base={base:x} {code:02x?}"), &code,
                    )
                    .segment(Segment::Ss, stack_segment(base, u32::MAX, big))
                    .segment(Segment::Fs, StoredSegment::unusable(0))
                    .register(Esp, 0x1234_9000, next_esp)
                    .register(Ebp, 0xabcd_8000, next_ebp)
                    .memory(base + destination, &vec![0xa5; frame.len()], ReadWrite)
                    .memory(base + source, &[0xef, 0xcd, 0xab, 0x89][..if word { 2 } else { 4 }], ReadOnly)
                    .expect_memory(base + destination, frame);
                    if default_word {
                        case = case.segment(Segment::Cs, code16());
                    }
                    if default_word || !big || base != 0 {
                        case = case.segmented_only();
                    }
                    cases.push(case);
                }
            }
        }
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

fn boundaries_and_aliases() -> Vec<Case> {
    let mut cases = vec![
        Case::preserving_flags(
            "word ENTER allocation borrows across 64K on a big stack",
            &[0x66, 0xc8, 0x20, 0, 0],
        )
        .register(Esp, 0x1234_0002, 0x1233_ffe0)
        .register(Ebp, 0xabcd_8000, 0xabcd_0000)
        .memory(0x1234_0000, &[0xa5; 2], ReadWrite)
        .memory(0x1233_ffe0, &[0x5a; 2], ReadWrite)
        .expect_memory(0x1234_0000, &[0, 0x80]),
        Case::preserving_flags(
            "word ENTER source borrow must not change final EBP upper half",
            &[0x66, 0xc8, 0, 0, 2],
        )
        .register(Esp, 0x9000, 0x8ffa)
        .register(Ebp, 0x1234_0000, 0x1234_8ffe)
        .memory(0x1233_fffe, &[0x34, 0x12], ReadOnly)
        .memory(0x8ffa, &[0xa5; 6], ReadWrite)
        .expect_memory(0x8ffa, &[0xfe, 0x8f, 0x34, 0x12, 0, 0]),
        Case::preserving_flags(
            "dword ENTER wraps each small-stack push and allocation separately",
            &[0xc8, 0x10, 0, 2],
        )
        .segmented_only()
        .segment(Segment::Ss, stack_segment(0, 0xffff, false))
        .register(Esp, 0xabcd_0004, 0xabcd_ffe8)
        .register(Ebp, 0x1234_0000, 0xabcd_0000)
        .memory(0, &[0xa5; 4], ReadWrite)
        .memory(
            0xfff8,
            &[0x44, 0x33, 0x22, 0x11, 0x78, 0x56, 0x34, 0x12],
            ReadWrite,
        )
        .expect_memory(0, &[0, 0, 0x34, 0x12])
        .expect_memory(0xfff8, &[0, 0, 0xcd, 0xab, 0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags(
            "ENTER display reads observe an earlier overlapping push",
            &[0xc8, 0, 0, 3],
        )
        .register(Esp, 0x9000, 0x8ff0)
        .register(Ebp, 0x9000, 0x8ffc)
        .memory(0x8ff0, &[0xa5; 16], ReadWrite)
        .expect_memory(
            0x8ff0,
            &[
                0xfc, 0x8f, 0, 0, 0, 0x90, 0, 0, 0, 0x90, 0, 0, 0, 0x90, 0, 0,
            ],
        ),
        Case::preserving_flags(
            "ENTER accesses only frame writes and final allocation, skipping intermediate pages",
            &[0xc8, 0xff, 0x7f, 0],
        )
        .register(Esp, 0x19004, 0x11001)
        .register(Ebp, 0xbeef_5678, 0x19000)
        .memory(0x19000, &[0xa5; 4], ReadWrite)
        .memory(0x11001, &[0x5a; 4], ReadWrite)
        .expect_memory(0x19000, &[0x78, 0x56, 0xef, 0xbe]),
        Case::preserving_flags(
            "ENTER allocation immediate is unsigned",
            &[0xc8, 0xff, 0xff, 0],
        )
        .register(Esp, 0x19004, 0x9001)
        .register(Ebp, 0xbeef_5678, 0x19000)
        .memory(0x19000, &[0xa5; 4], ReadWrite)
        .memory(0x9001, &[0x5a; 4], ReadWrite)
        .expect_memory(0x19000, &[0x78, 0x56, 0xef, 0xbe]),
        Case::preserving_flags(
            "ENTER first push can cross a page boundary",
            &[0xc8, 0, 0, 0],
        )
        .register(Esp, 0x5002, 0x4ffe)
        .register(Ebp, 0x1234_5678, 0x4ffe)
        .memory(0x4ffe, &[0xa5; 4], ReadWrite)
        .expect_memory(0x4ffe, &[0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags(
            "ENTER display reads observe aliased physical stack pages",
            &[0xc8, 0, 0, 3],
        )
        .register(Esp, 0x9000, 0x8ff0)
        .register(Ebp, 0xa000, 0x8ffc)
        .map_page(8, 0x8000, ReadWrite)
        .map_page(9, 0x8000, ReadOnly)
        .memory(0x8ff0, &[0xa5; 16], ReadWrite)
        .expect_memory(
            0x8ff0,
            &[
                0xfc, 0x8f, 0, 0, 0, 0xa0, 0, 0, 0, 0xa0, 0, 0, 0, 0xa0, 0, 0,
            ],
        ),
        Case::preserving_flags(
            "ENTER retains the established full-size segment wrap",
            &[0xc8, 0, 0, 0],
        )
        .register(Esp, 2, 0xffff_fffe)
        .register(Ebp, 0x1234_5678, 0xffff_fffe)
        .memory(0xffff_fffe, &[0xa5; 2], ReadWrite)
        .memory(0, &[0xa5; 2], ReadWrite)
        .expect_memory(0xffff_fffe, &[0x78, 0x56])
        .expect_memory(0, &[0x34, 0x12]),
    ];
    let code = [vec![0x66; 11], vec![0xc8, 0x20, 0, 0]].concat();
    cases.push(
        Case::preserving_flags(
            "ENTER finishes its nesting byte at instruction byte fifteen",
            &code,
        )
        .at(0x1ff1)
        .register(Esp, 0x9000, 0x8fde)
        .register(Ebp, 0xbeef_5678, 0xbeef_8ffe)
        .memory(0x8ffe, &[0xa5; 2], ReadWrite)
        .expect_memory(0x8ffe, &[0x78, 0x56]),
    );
    cases
}

test_cases!(independent_operand_and_stack_widths, widths());
test_cases!(nesting_levels_and_masked_immediate, nesting_levels());
test_cases!(
    stack_boundaries_and_overlapping_frames,
    boundaries_and_aliases()
);
