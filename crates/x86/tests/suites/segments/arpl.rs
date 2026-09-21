//! ARPL compares RPLs as unsigned values, writes a word and changes only ZF.

use super::data;
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use crate::{CpuState, Gpr32::*, Segment, SegmentAttributes, StoredFlags, StoredSegment};

fn initial_flags(zf: bool) -> Flags<bool> {
    Flags {
        cf: true,
        pf: false,
        af: true,
        zf,
        sf: false,
        of: true,
    }
}

fn zero_flag(set: bool) -> Flags<FlagExpectation> {
    Flags {
        zf: if set { Set } else { Clear },
        ..Flags::all(Preserved)
    }
}

fn pending_add() -> StoredFlags {
    let mut flags = CpuState::filled(0xa5).flags;
    flags.status_source.kind = 10;
    flags.status_source.left = 0x7fff_ffff;
    flags.status_source.right = 1;
    flags
}

fn rpl_pairs() -> Vec<Case> {
    // Destination RPL rows, source RPL columns; literal cells are (new RPL, ZF).
    let outcomes = [
        [(0, false), (1, true), (2, true), (3, true)],
        [(1, false), (1, false), (2, true), (3, true)],
        [(2, false), (2, false), (2, false), (3, true)],
        [(3, false), (3, false), (3, false), (3, false)],
    ];
    let mut cases = Vec::new();
    for (destination_rpl, row) in outcomes.into_iter().enumerate() {
        for (source_rpl, (rpl, adjusted)) in row.into_iter().enumerate() {
            cases.push(
                Case::new(
                    format!("ARPL destination RPL {destination_rpl}, source RPL {source_rpl}"),
                    &[0x63, 0xc8],
                    initial_flags(!adjusted),
                    zero_flag(adjusted),
                )
                .register(Eax, 0xabcd_8004 | destination_rpl as u32, 0xabcd_8004 | rpl)
                .initial_register(Ecx, 0x9876_fff8 | source_rpl as u32),
            );
        }
    }
    cases
}

fn fixed_word_forms() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code16, code) in [
        (false, &[0x63, 0xc8][..]),
        (false, &[0x66, 0x63, 0xc8][..]),
        (true, &[0x63, 0xc8][..]),
        (true, &[0x66, 0x63, 0xc8][..]),
    ] {
        let mut case = Case::new(
            format!("null selector adjusts as a word, code16 {code16}, {code:02x?}"),
            code,
            initial_flags(false),
            zero_flag(true),
        )
        .register(Eax, 0xabcd_0000, 0xabcd_0003)
        .initial_register(Ecx, 0x9876_0003);
        if code16 {
            case = case.segmented_only().segment(
                Segment::Cs,
                StoredSegment {
                    attributes: SegmentAttributes::from_bits(0x07),
                    ..StoredSegment::flat_code32(0x1b)
                },
            );
        }
        cases.push(case);
    }
    cases.push(
        Case::new(
            "self-aliasing compares the original selector to itself",
            &[0x63, 0xff],
            initial_flags(true),
            zero_flag(false),
        )
        .initial_register(Edi, 0xabcd_8023),
    );
    cases
}

#[rustfmt::skip]
fn memory_forms() -> Vec<Case> {
    vec![
        Case::new("operand override still writes only the final mapped word", &[0x66, 0x63, 0x0b],
            initial_flags(false), zero_flag(true))
            .initial_registers(&[(Ebx, 0x4ffe), (Ecx, 0xabcd_0003)])
            .memory(0x4ffd, &[0x5a, 0xfc, 0xff], ReadWrite).expect_memory(0x4ffe, &[0xff, 0xff]),
        Case::new("source BX also supplies the destination address", &[0x63, 0x1b],
            initial_flags(false), zero_flag(true))
            .initial_register(Ebx, 0x4003).memory(0x4002, &[0x5a, 0x20, 0xf3, 0x5a], ReadWrite)
            .expect_memory(0x4003, &[0x23, 0xf3]),
        Case::new("FS override applies to a split word at a 16-bit effective address", &[0x64, 0x67, 0x63, 0x42, 0],
            initial_flags(false), zero_flag(true)).segmented_only()
            .initial_registers(&[(Eax, 3), (Ebp, 0xabcd_fffe), (Esi, 0xdead_0001)])
            .segment(Segment::Fs, data(0x8000, 0x10000)).segment(Segment::Ss, StoredSegment::unusable(0))
            .map_page(0x17, 0x8000, ReadWrite).map_page(0x18, 0xa000, ReadWrite)
            .memory(0x17ffe, &[0x5a, 0xfc, 0xff, 0x5a], ReadWrite).expect_memory(0x17fff, &[0xff, 0xff]),
    ]
}

#[rustfmt::skip]
fn faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("absent destination reports a write fault", &[0x63, 0x0b]).stored_flags(pending_add())
            .initial_registers(&[(Ebx, 0x4020), (Ecx, 3)]).fault(0x4020, 2),
        Case::preserving_flags("an adjustment requires write permission before replacing ZF", &[0x63, 0x0b])
            .stored_flags(pending_add()).initial_registers(&[(Ebx, 0x4020), (Ecx, 3)])
            .memory(0x4020, &[0x20, 0], ReadOnly).fault(0x4020, 3),
        Case::preserving_flags("equal RPL still requires both bytes to be writable", &[0x63, 0x0b])
            .stored_flags(pending_add()).initial_registers(&[(Ebx, 0x4fff), (Ecx, 3)])
            .memory(0x4fff, &[0x23], ReadWrite).memory(0x5000, &[0], ReadOnly).fault(0x5000, 3),
        Case::preserving_flags("greater destination RPL still requires the second page", &[0x63, 0x0b])
            .stored_flags(pending_add()).initial_registers(&[(Ebx, 0x4fff), (Ecx, 1)])
            .memory(0x4fff, &[0x23], ReadWrite).fault(0x5000, 2),
        Case::preserving_flags("DS write denial precedes paging even for a no-change result", &[0x63, 0x0b])
            .stored_flags(pending_add()).segmented_only().initial_registers(&[(Ebx, 0xfff), (Ecx, 3)])
            .segment(Segment::Ds, data(0x4000, 0xfff)).memory(0x4fff, &[0x23], ReadWrite).general_protection(0),
        Case::preserving_flags("SS write denial preserves the entire incoming flag record", &[0x63, 0x0c, 0x24])
            .stored_flags(pending_add()).segmented_only().initial_registers(&[(Esp, 0xfff), (Ecx, 3)])
            .segment(Segment::Ss, data(0x4000, 0xfff)).memory(0x4fff, &[0x20], ReadWrite).stack_fault(0),
    ]
}

#[rustfmt::skip]
fn history() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("adjustment and no-change ZF feed consumers before a later write fault")
            .initial_registers(&[(Eax, 0x7fff_ffff), (Ecx, 3), (Edx, 0xaabb_cc00), (Ebx, 0xdddd_eeff), (Esi, 0x4000)])
            .step(Step::new(&[0x83, 0xc0, 1], Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x8000_0000))
            .step(Step::new(&[0x63, 0xc8], zero_flag(true)).register(Eax, 0x8000_0003))
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc2]).register(Edx, 0xaabb_cc01))
            .step(Step::new(&[0x63, 0xc8], zero_flag(false)))
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc3]).register(Ebx, 0xdddd_ee00))
            .step(Step::preserving_flags(&[0x63, 0x0e]).fault(0x4000, 2))
            .trailing_code(&[0xb0, 0x99], 1),
    ]
}

test_cases!(unsigned_rpl_comparisons, rpl_pairs());
test_cases!(fixed_word_width_and_aliases, fixed_word_forms());
test_cases!(memory_operands, memory_forms());
test_cases!(write_faults_preserve_flags_and_memory, faults());
test_sequences!(flags_consumers_and_fault_publication, history());
