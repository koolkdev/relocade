use super::data;
use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    CpuState,
    Gpr32::{Eax, Ecx, Edi, Esi},
    Segment, StoredFlags, StoredSegment,
};

fn record(backward: bool) -> StoredFlags {
    let mut flags = CpuState::filled(0xa5).flags;
    flags.bytes.df = u8::from(backward);
    flags
}

fn selection() -> Vec<Case> {
    vec![
        Case::preserving_flags("MOVSB reads DS and writes ES", &[0xa4])
            .interpreter_only()
            .stored_flags(record(false))
            .segment(Segment::Ds, data(0x8000, 0xff))
            .segment(Segment::Es, data(0x4000, 0xff))
            .register(Esi, 0x20, 0x21)
            .register(Edi, 0x30, 0x31)
            .memory(0x8020, &[0x78], ReadOnly)
            .memory(0x4030, &[0xff], ReadWrite)
            .expect_memory(0x4030, &[0x78]),
        Case::preserving_flags("FS MOVSB overrides only the source", &[0x64, 0xa4])
            .interpreter_only()
            .stored_flags(record(false))
            .segment(Segment::Ds, StoredSegment::unusable(0x23))
            .segment(Segment::Fs, data(0x8000, 0xff))
            .segment(Segment::Es, data(0x4000, 0xff))
            .register(Esi, 0x20, 0x21)
            .register(Edi, 0x30, 0x31)
            .memory(0x8020, &[0x78], ReadOnly)
            .memory(0x4030, &[0xff], ReadWrite)
            .expect_memory(0x4030, &[0x78]),
        Case::preserving_flags(
            "GS LODSD adds the source base and keeps a full ESI",
            &[0x65, 0xad],
        )
        .stored_flags(record(false))
        .segment(Segment::Gs, data(0x8000, 0x1ffff))
        .register(Esi, 0x10020, 0x10024)
        .register(Eax, 0, 0x1234_5678)
        .memory(0x18020, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::preserving_flags("STOSB ignores an unusable source override", &[0x64, 0xaa])
            .stored_flags(record(false))
            .segment(Segment::Fs, StoredSegment::unusable(0x53))
            .initial_register(Eax, 0x78)
            .register(Edi, 0x4000, 0x4001)
            .memory(0x4000, &[0xff], ReadWrite)
            .expect_memory(0x4000, &[0x78]),
        Case::replacing_flags(
            "CMPSB compares FS source against fixed ES destination",
            &[0x64, 0xa6],
            Flags {
                cf: Clear,
                pf: Set,
                af: Clear,
                zf: Set,
                sf: Clear,
                of: Clear,
            },
        )
        .stored_flags(record(false))
        .segment(Segment::Fs, data(0x8000, 0xff))
        .register(Esi, 0x20, 0x21)
        .register(Edi, 0x4000, 0x4001)
        .memory(0x8020, &[0x78], ReadOnly)
        .memory(0x4000, &[0x78], ReadOnly),
        Case::replacing_flags(
            "SCASB ignores an unusable source override",
            &[0x64, 0xae],
            Flags {
                cf: Clear,
                pf: Set,
                af: Clear,
                zf: Set,
                sf: Clear,
                of: Clear,
            },
        )
        .stored_flags(record(false))
        .segment(Segment::Fs, StoredSegment::unusable(0x53))
        .initial_register(Eax, 0x78)
        .register(Edi, 0x4000, 0x4001)
        .memory(0x4000, &[0x78], ReadOnly),
    ]
}

fn faults_and_repetition() -> Vec<Case> {
    let mut cases = Vec::new();
    for code in [&[0x64, 0xa4][..], &[0x64, 0xa6], &[0x64, 0xae]] {
        cases.push(
            Case::preserving_flags(
                format!("ES fault preserves string indices and flags {code:02x?}"),
                code,
            )
            .interpreter_only()
            .stored_flags(record(false))
            .segment(Segment::Fs, data(0x8000, 0xff))
            .segment(Segment::Es, StoredSegment::unusable(0x23))
            .initial_registers(&[(Esi, 0x20), (Edi, 0x4000)])
            .memory(0x8020, &[0x78], ReadOnly)
            .general_protection(0),
        );
    }
    for code in [
        &[0x64, 0xf3, 0xa4][..],
        &[0xf3, 0x64, 0xaa],
        &[0x66, 0x64, 0xf3, 0xa5],
    ] {
        cases.push(
            Case::preserving_flags(
                format!("zero REP count performs no segment access {code:02x?}"),
                code,
            )
            .interpreter_only()
            .stored_flags(record(false))
            .segment(Segment::Ds, StoredSegment::unusable(0x23))
            .segment(Segment::Es, StoredSegment::unusable(0x23))
            .segment(Segment::Fs, StoredSegment::unusable(0x53))
            .initial_register(Ecx, 0),
        );
    }
    for (backward, esi, next_esi, edi, next_edi) in [
        (false, 0, 2, 0x4000, 0x4002),
        (true, 1, u32::MAX, 0x4001, 0x3fff),
    ] {
        cases.push(
            Case::preserving_flags(
                format!(
                    "REP source segment fault retains completed iterations, backward {backward}"
                ),
                &[0xf3, 0x64, 0xa4],
            )
            .stored_flags(record(backward))
            .instruction_count(7)
            .segment(Segment::Fs, data(0x8000, 1))
            .register(Ecx, 3, 1)
            .register(Esi, esi, next_esi)
            .register(Edi, edi, next_edi)
            .memory(0x8000, &[0x11, 0x22], ReadOnly)
            .memory(0x4000, &[0xff; 2], ReadWrite)
            .expect_memory(0x4000, &[0x11, 0x22])
            .general_protection(0),
        );
    }
    cases.push(
        Case::preserving_flags(
            "REP destination segment fault retains complete word copies",
            &[0x64, 0x66, 0xf3, 0xa5],
        )
        .interpreter_only()
        .stored_flags(record(false))
        .segment(Segment::Fs, data(0x8000, 0xff))
        .segment(Segment::Es, data(0x4000, 3))
        .register(Ecx, 3, 1)
        .register(Esi, 0, 4)
        .register(Edi, 0, 4)
        .memory(0x8000, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66], ReadOnly)
        .memory(0x4000, &[0xff; 6], ReadWrite)
        .expect_memory(0x4000, &[0x11, 0x22, 0x33, 0x44])
        .general_protection(0),
    );
    cases
}

test_cases!(
    sources_accept_overrides_and_destinations_use_es,
    selection()
);
test_cases!(
    faults_preserve_string_and_rep_restart_boundaries,
    faults_and_repetition()
);
