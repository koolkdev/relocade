//! Paging proofs must follow current addresses, widths and instruction progress.

use crate::support::{
    cases::Permissions::{ReadOnly, ReadWrite},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::{CpuState, Gpr32::*, Segment, StoredSegment};

fn accesses() -> Vec<Case> {
    vec![
        Case::preserving_flags("a later wider read checks the next page")
            .initial_registers(&[(Esi, 0x4fff), (Eax, 0)])
            .memory(0x4fff, &[0x11], ReadOnly)
            .step(Step::preserving_flags(&[0x8a, 0x06]).register(Eax, 0x11))
            .step(Step::preserving_flags(&[0x66, 0x8b, 0x06]).fault(0x5000, 0)),
        Case::preserving_flags("a partial address-register write selects a new page")
            .initial_register(Esi, 0x4000)
            .memory(0x4000, &[0x11, 0x22, 0x33, 0x44], ReadOnly)
            .step(Step::preserving_flags(&[0x8b, 0x06]).register(Eax, 0x4433_2211))
            .step(Step::preserving_flags(&[0x66, 0xbe, 0, 0x50]).register(Esi, 0x5000))
            .step(Step::preserving_flags(&[0x8b, 0x06]).fault(0x5000, 0)),
        Case::preserving_flags("a later paging fault retains an earlier store")
            .initial_registers(&[(Esi, 0x4ffc), (Eax, 0x4433_2211)])
            .memory(0x4ffc, &[0; 4], ReadWrite)
            .step(
                Step::preserving_flags(&[0x89, 0x06])
                    .expect_memory(0x4ffc, &[0x11, 0x22, 0x33, 0x44]),
            )
            .step(Step::preserving_flags(&[0x89, 0x46, 4]).fault(0x5000, 2)),
        Case::preserving_flags("a cached page never bypasses the next segment check")
            .segmented_only()
            .segment(
                Segment::Ds,
                StoredSegment {
                    limit: 2,
                    base: 0x4000,
                    ..StoredSegment::flat_data32(0x23)
                },
            )
            .initial_registers(&[(Esi, 0), (Eax, 0)])
            .memory(0x4000, &[0x11, 0x22, 0x33, 0x44], ReadOnly)
            .step(Step::preserving_flags(&[0x8a, 0x06]).register(Eax, 0x11))
            .step(Step::preserving_flags(&[0x8b, 0x06]).general_protection(0)),
        Case::preserving_flags("a skipped REP cannot authorize a later read")
            .initial_registers(&[(Ecx, 0), (Esi, 0x4000), (Edi, 0x6000)])
            .step(Step::preserving_flags(&[0xf3, 0xa4]))
            .step(Step::preserving_flags(&[0x8a, 0x06]).fault(0x4000, 0)),
    ]
}

test_sequences!(ordered_accesses, accesses());

fn repeated_access() -> Vec<Case> {
    let mut flags = CpuState::filled(0xa5).flags;
    flags.bytes.df = 0;
    vec![
        Case::preserving_flags("REP inherits a page proof but checks each changing source")
            .stored_flags(flags)
            .initial_registers(&[(Eax, 0), (Ecx, 2), (Esi, 0x4fff), (Edi, 0x6000)])
            .memory(0x4fff, &[0x11], ReadOnly)
            .memory(0x6000, &[0, 0], ReadWrite)
            .step(Step::preserving_flags(&[0x8a, 0x06]).register(Eax, 0x11))
            .step(
                Step::preserving_flags(&[0xf3, 0xa4])
                    .register(Ecx, 1)
                    .register(Esi, 0x5000)
                    .register(Edi, 0x6001)
                    .expect_memory(0x6000, &[0x11])
                    .fault(0x5000, 0),
            ),
    ]
}

test_sequences!(repeated_access_uses_current_address, repeated_access());
