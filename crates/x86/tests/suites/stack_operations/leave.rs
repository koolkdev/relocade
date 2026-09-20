use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};
use wasm86_x86::{
    Gpr32::{Ebp, Esp},
    Segment, SegmentAttributes, StoredSegment,
};

#[path = "leave/faults.rs"]
mod faults;
#[path = "leave/progress.rs"]
mod progress;

fn stack_segment(base: u32, limit: u32, big: bool) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        selector: 0x23,
        attributes: SegmentAttributes::from_bits(if big { 0x15 } else { 0x05 }),
    }
}

fn code16() -> StoredSegment {
    StoredSegment {
        attributes: SegmentAttributes::from_bits(0x07),
        ..StoredSegment::flat_code32(0x1b)
    }
}

fn widths() -> Vec<Case> {
    let mut cases = Vec::new();
    for (big, word, offset, next_esp, next_ebp) in [
        (true, false, 0x1234_fffc, 0x1235_0000, 0x89ab_cdef),
        (true, true, 0x1234_fffc, 0x1234_fffe, 0x1234_cdef),
        (false, false, 0xfffc, 0xabcd_0000, 0x89ab_cdef),
        (false, true, 0xfffc, 0xabcd_fffe, 0x1234_cdef),
    ] {
        for default_word in [false, true] {
            for ignored in [&[][..], &[0x64, 0x67]] {
                for base in [0u32, 0x20_0000] {
                    let mut code = ignored.to_vec();
                    if word != default_word {
                        code.push(0x66);
                    }
                    code.push(0xc9);
                    let mut case = Case::preserving_flags(
                        format!("LEAVE B={big} word={word} code16={default_word} base={base:x} {code:02x?}"),
                        &code,
                    )
                    .segment(Segment::Ss, stack_segment(base, u32::MAX, big))
                    .segment(Segment::Fs, StoredSegment::unusable(0))
                    .register(Esp, 0xabcd_3000, next_esp)
                    .register(Ebp, 0x1234_fffc, next_ebp)
                    .memory(
                        base + offset,
                        &[0xef, 0xcd, 0xab, 0x89][..if word { 2 } else { 4 }],
                        ReadOnly,
                    );
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

fn boundaries() -> Vec<Case> {
    let mut cases = vec![
        Case::preserving_flags(
            "word LEAVE reads exactly the last two bytes of a page",
            &[0x66, 0xc9],
        )
        .register(Esp, 0xdead_beef, 0x5000)
        .register(Ebp, 0x4ffe, 0xcdef)
        .memory(0x4ffe, &[0xef, 0xcd], ReadOnly),
        Case::preserving_flags("dword LEAVE reads across a page boundary", &[0xc9])
            .register(Esp, 0xdead_beef, 0x5002)
            .register(Ebp, 0x4ffe, 0x89ab_cdef)
            .memory(0x4ffe, &[0xef, 0xcd, 0xab, 0x89], ReadOnly),
        Case::preserving_flags(
            "LEAVE discards an out-of-limit entry stack pointer",
            &[0xc9],
        )
        .segmented_only()
        .segment(Segment::Ss, stack_segment(0x20000, 0x8003, true))
        .register(Esp, 0xffff_ffff, 0x8004)
        .register(Ebp, 0x8000, 0x89ab_cdef)
        .memory(0x28000, &[0xef, 0xcd, 0xab, 0x89], ReadOnly),
        Case::preserving_flags("LEAVE wraps full ESP after the final dword", &[0xc9])
            .register(Esp, 0x1234_5000, 0)
            .register(Ebp, 0xffff_fffc, 0x89ab_cdef)
            .memory(0xffff_fffc, &[0xef, 0xcd, 0xab, 0x89], ReadOnly),
        Case::preserving_flags(
            "LEAVE permits the established full-size segment wrap",
            &[0xc9],
        )
        .register(Esp, 0x1234_5000, 2)
        .register(Ebp, 0xffff_fffe, 0x89ab_cdef)
        .memory(0xffff_fffe, &[0xef, 0xcd], ReadOnly)
        .memory(0, &[0xab, 0x89], ReadOnly),
        Case::preserving_flags("LEAVE applies SS base before linear wrap", &[0xc9])
            .segmented_only()
            .segment(Segment::Ss, stack_segment(0xffff_b000, 0xffff, true))
            .register(Esp, 0x8000, 0x5002)
            .register(Ebp, 0x4ffe, 0x89ab_cdef)
            .memory(0xffff_fffe, &[0xef, 0xcd], ReadOnly)
            .memory(0, &[0xab, 0x89], ReadOnly),
    ];
    for (word, bp, limit, next_esp, next_ebp) in [
        (true, 0xfffe, 0xffff, 0xabcd_0000, 0xbeef_cdef),
        (false, 0xfffc, 0xffff, 0xabcd_0000, 0x89ab_cdef),
        (false, 0xfffe, 0x10001, 0xabcd_0002, 0x89ab_cdef),
    ] {
        let code = if word { vec![0x66, 0xc9] } else { vec![0xc9] };
        cases.push(
            Case::preserving_flags(
                format!("LEAVE keeps stack-frame bytes consecutive at BP={bp:x}"),
                &code,
            )
            .segmented_only()
            .segment(Segment::Ss, stack_segment(0, limit, false))
            .register(Esp, 0xabcd_3000, next_esp)
            .register(Ebp, 0xbeef_0000 | bp, next_ebp)
            .memory(
                bp,
                &[0xef, 0xcd, 0xab, 0x89][..if word { 2 } else { 4 }],
                ReadOnly,
            ),
        );
    }
    let code = [vec![0x66; 14], vec![0xc9]].concat();
    cases.push(
        Case::preserving_flags(
            "LEAVE can finish at instruction byte fifteen and the code page end",
            &code,
        )
        .at(0x1ff1)
        .register(Esp, 0x6000, 0x9002)
        .register(Ebp, 0x9000, 0xcdef)
        .memory(0x9000, &[0xef, 0xcd], ReadOnly),
    );
    cases
}

test_cases!(independent_code_operand_and_stack_widths, widths());
test_cases!(stack_frame_boundaries, boundaries());
