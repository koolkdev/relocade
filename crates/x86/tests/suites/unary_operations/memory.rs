use wasm86_x86::{
    CpuState,
    Gpr32::{Ebx, Ecx},
    StoredFlags,
};
use wasm86_x86::{FlagBytes, StoredStatusSource};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};

const SAVED_CARRY: StoredFlags = StoredFlags {
    status_source: StoredStatusSource {
        kind: 2,
        reserved: [0xa5; 3],
        left: 0xff,
        right: 1,
    },
    bytes: FlagBytes {
        cf: 0,
        pf: 0xa5,
        af: 0xa5,
        zf: 0xa5,
        sf: 0xa5,
        of: 0xa5,
        tf: 0xa5,
        df: 0xa5,
        nt: 0xa5,
        ac: 0xa5,
        id: 0xa5,
        reserved: 0xa5,
    },
};
const INITIAL: Flags<bool> = Flags {
    cf: true,
    pf: true,
    af: true,
    zf: true,
    sf: false,
    of: false,
};
const INVALID: StoredFlags = StoredFlags {
    status_source: StoredStatusSource {
        kind: 0xff,
        ..SAVED_CARRY.status_source
    },
    ..SAVED_CARRY
};

#[rustfmt::skip]
fn memory_updates() -> Vec<Case> {
    let mut cases = Vec::new();
    for next_frame in [0x9000, 0xa000] {
        for (case, before, after) in [
            (Case::new(format!("INC byte memory, second frame {next_frame:x}"), &[0xfe, 0x03], INITIAL,
                Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).stored_flags(SAVED_CARRY),
                &[0xff][..], &[0][..]),
            (Case::new(format!("INC word memory, second frame {next_frame:x}"), &[0x66, 0xff, 0x03], INITIAL,
                Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).stored_flags(SAVED_CARRY),
                &[0xff, 0xff][..], &[0, 0][..]),
            (Case::new(format!("INC dword memory, second frame {next_frame:x}"), &[0xff, 0x03], INITIAL,
                Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).stored_flags(SAVED_CARRY),
                &[0xff, 0xff, 0xff, 0xff][..], &[0, 0, 0, 0][..]),
            (Case::new(format!("DEC byte memory, second frame {next_frame:x}"), &[0xfe, 0x0b], INITIAL,
                Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).stored_flags(SAVED_CARRY),
                &[0][..], &[0xff][..]),
            (Case::new(format!("DEC word memory, second frame {next_frame:x}"), &[0x66, 0xff, 0x0b], INITIAL,
                Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).stored_flags(SAVED_CARRY),
                &[0, 0][..], &[0xff, 0xff][..]),
            (Case::new(format!("DEC dword memory, second frame {next_frame:x}"), &[0xff, 0x0b], INITIAL,
                Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).stored_flags(SAVED_CARRY),
                &[0, 0, 0, 0][..], &[0xff, 0xff, 0xff, 0xff][..]),
            (Case::preserving_flags(format!("NOT byte memory, second frame {next_frame:x}"), &[0xf6, 0x13]).stored_flags(INVALID),
                &[0x0f][..], &[0xf0][..]),
            (Case::preserving_flags(format!("NOT word memory, second frame {next_frame:x}"), &[0x66, 0xf7, 0x13]).stored_flags(INVALID),
                &[0x0f, 0x0f][..], &[0xf0, 0xf0][..]),
            (Case::preserving_flags(format!("NOT dword memory, second frame {next_frame:x}"), &[0xf7, 0x13]).stored_flags(INVALID),
                &[0x0f, 0x0f, 0x0f, 0x0f][..], &[0xf0, 0xf0, 0xf0, 0xf0][..]),
            (Case::replacing_flags(format!("NEG byte memory, second frame {next_frame:x}"), &[0xf6, 0x1b],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).stored_flags(INVALID),
                &[1][..], &[0xff][..]),
            (Case::replacing_flags(format!("NEG word memory, second frame {next_frame:x}"), &[0x66, 0xf7, 0x1b],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).stored_flags(INVALID),
                &[1, 0][..], &[0xff, 0xff][..]),
            (Case::replacing_flags(format!("NEG dword memory, second frame {next_frame:x}"), &[0xf7, 0x1b],
                Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).stored_flags(INVALID),
                &[1, 0, 0, 0][..], &[0xff, 0xff, 0xff, 0xff][..]),
        ] {
            cases.push(case.initial_register(Ebx, 0x4fff)
                .map_page(4, 0x8000, ReadWrite).map_page(5, next_frame, ReadWrite)
                .backing(0x8ffe, &[0xa5, before[0]]).backing(next_frame, &before[1..])
                .backing(next_frame + before.len() as u32 - 1, &[0x5a])
                .expect_memory(0x4fff, after));
        }
    }
    cases
}

#[rustfmt::skip]
fn access_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for code in [
        &[0xfe, 0x03][..], &[0xfe, 0x0b], &[0xf6, 0x13], &[0xf6, 0x1b],
        &[0x66, 0xff, 0x03], &[0x66, 0xff, 0x0b], &[0x66, 0xf7, 0x13], &[0x66, 0xf7, 0x1b],
        &[0xff, 0x03], &[0xff, 0x0b], &[0xf7, 0x13], &[0xf7, 0x1b],
    ] {
        for (first, second, address, error) in [
            (None, None, 0x4fff, 2), (Some(ReadOnly), None, 0x4fff, 3),
            (Some(ReadWrite), None, 0x5000, 2), (Some(ReadWrite), Some(ReadOnly), 0x5000, 3),
        ] {
            if matches!(code[0], 0xfe | 0xf6) && address == 0x5000 { continue; }
            let mut case = Case::new(format!("unary {code:02x?}, fault {address:#x}/{error}"), code,
                Flags::all(true), Flags::all(Preserved))
                .stored_flags(StoredFlags {
                    status_source: StoredStatusSource {
                        kind: 0,
                        ..(CpuState::filled(0xa5).flags).status_source
                    },
                    ..CpuState::filled(0xa5).flags
                }).preserve_flag_record()
                .initial_register(Ebx, 0x4fff)
                .backing(0x8ffe, &[0xa5, 0xff]).backing(0xa000, &[0xff, 0xff, 0xff, 0x5a])
                .fault(address, error);
            if let Some(permissions) = first { case = case.map_page(4, 0x8000, permissions); }
            if let Some(permissions) = second { case = case.map_page(5, 0xa000, permissions); }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn wrapping_ranges() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, address) in [
        (&[0x66, 0xff, 0x03][..], 0xffff_ffff), (&[0x66, 0xff, 0x0b][..], 0xffff_ffff),
        (&[0x66, 0xf7, 0x13][..], 0xffff_ffff), (&[0x66, 0xf7, 0x1b][..], 0xffff_ffff),
        (&[0xff, 0x03][..], 0xffff_fffe), (&[0xff, 0x0b][..], 0xffff_fffe),
        (&[0xf7, 0x13][..], 0xffff_fffe), (&[0xf7, 0x1b][..], 0xffff_fffe),
    ] {
        for first in [ReadWrite, ReadOnly] {
            cases.push(Case::new(format!("unary {code:02x?} wrapping range, first page {first:?}"), code,
                Flags::all(true), Flags::all(Preserved))
                .stored_flags(StoredFlags {
                    status_source: StoredStatusSource {
                        kind: 0,
                        ..(CpuState::filled(0xa5).flags).status_source
                    },
                    ..CpuState::filled(0xa5).flags
                }).preserve_flag_record()
                .initial_register(Ebx, address)
                .map_page(0xfffff, 0x8000, first)
                .backing(0x8ffe, &[0x11, 0x22]).backing(0xa000, &[0x33, 0x44])
                .fault(if first == ReadOnly { address } else { 0 }, if first == ReadOnly { 3 } else { 2 }));
        }
    }
    cases
}

#[rustfmt::skip]
fn completed_stores_before_faults() -> Vec<SequenceCase> {
    vec![
        SequenceCase::new("completed dword INC store precedes later fault", INITIAL).stored_flags(SAVED_CARRY)
            .initial_register(Ebx, 0x4000).initial_register(Ecx, 0x6000)
            .map_page(4, 0x8000, ReadWrite).backing(0x7fff, &[0xa5, 0xff, 0xff, 0xff, 0xff, 0x5a])
            .step(Checkpoint::new(&[0xff, 0x03],
                Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .expect_memory(0x4000, &[0, 0, 0, 0]))
            .step(Checkpoint::preserving_flags(&[0xff, 0x09]).fault(0x6000, 2)),
        SequenceCase::new("mixed-width unary stores and saved carry precede later fault", INITIAL).stored_flags(SAVED_CARRY)
            .initial_register(Ebx, 0x4fff).initial_register(Ecx, 0x6000)
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0xa000, ReadWrite)
            .backing(0x8ffe, &[0xa5, 0xff]).backing(0xa000, &[0, 0x5a])
            .step(Checkpoint::new(&[0xfe, 0x03],
                Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .expect_memory(0x4fff, &[0]))
            .step(Checkpoint::new(&[0x66, 0xff, 0x0b],
                Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
                .expect_memory(0x4fff, &[0xff, 0xff]))
            .step(Checkpoint::preserving_flags(&[0xff, 0x01]).fault(0x6000, 2)),
    ]
}

test_cases!(operand_widths_and_page_layouts, memory_updates());
test_cases!(faults_preserve_flags_and_memory, access_faults());
test_cases!(
    wrapped_operands_check_real_page_permissions,
    wrapping_ranges()
);
test_sequences!(
    completed_effects_precede_faults,
    completed_stores_before_faults()
);
