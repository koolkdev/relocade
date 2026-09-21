//! Completed repetition supplies state to later instructions in the same block.

use super::{flags, record};
use crate::support::{
    cases::Permissions::{ReadOnly, ReadWrite},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edi, Esi};

fn successors() -> Vec<Case> {
    let mut cases = vec![
        Case::preserving_flags(
            "address-sized REP results feed full-register aliases and memory reads",
        )
        .stored_flags(record(0))
        .initial_registers(&[(Esi, 0xeeee_4000), (Edi, 0xdddd_6000)])
        .memory(0x4000, &[0x11, 0x22], ReadOnly)
        .memory(0x6000, &[0xaa; 4], ReadWrite)
        .step(Step::preserving_flags(&[0xb9, 2, 0, 0xcd, 0xab]).register(Ecx, 0xabcd_0002))
        .step(
            Step::preserving_flags(&[0x67, 0xf3, 0xa4])
                .register(Ecx, 0xabcd_0000)
                .register(Esi, 0xeeee_4002)
                .register(Edi, 0xdddd_6002)
                .expect_memory(0x6000, &[0x11, 0x22]),
        )
        .step(Step::preserving_flags(&[0x89, 0xcb]).register(Ebx, 0xabcd_0000))
        .step(Step::preserving_flags(&[0x89, 0xf0]).register(Eax, 0xeeee_4002))
        .step(Step::preserving_flags(&[0x89, 0xf9]).register(Ecx, 0xdddd_6002))
        .step(Step::preserving_flags(&[0xa1, 0, 0x60, 0, 0]).register(Eax, 0xaaaa_2211)),
        Case::preserving_flags("a successor data fault retains completed REP state and stores")
            .stored_flags(record(0))
            .initial_registers(&[(Ecx, 2), (Esi, 0x4000), (Edi, 0x6000), (Ebx, 0x9000)])
            .memory(0x4000, &[0x11, 0x22], ReadOnly)
            .memory(0x6000, &[0xaa; 2], ReadWrite)
            .step(
                Step::preserving_flags(&[0xf3, 0xa4])
                    .register(Ecx, 0)
                    .register(Esi, 0x4002)
                    .register(Edi, 0x6002)
                    .expect_memory(0x6000, &[0x11, 0x22]),
            )
            .step(Step::preserving_flags(&[0x8b, 0x03]).fault(0x9000, 0)),
        Case::preserving_flags("a REP element fault prevents its compiled successor store")
            .stored_flags(record(0))
            .initial_registers(&[(Ecx, 2), (Esi, 0x4fff), (Edi, 0x6000)])
            .map_page(4, 0x8000, ReadOnly)
            .backing(0x8fff, &[0x11])
            .memory(0x6000, &[0xaa; 2], ReadWrite)
            .step(
                Step::preserving_flags(&[0xf3, 0xa4])
                    .register(Ecx, 1)
                    .register(Esi, 0x5000)
                    .register(Edi, 0x6001)
                    .expect_memory(0x6000, &[0x11])
                    .fault(0x5000, 0),
            )
            .trailing_code(&[0xc6, 0x05, 0, 0x60, 0, 0, 0x99], 1),
    ];
    for count in [0, 3] {
        let scan = if count == 0 {
            Step::preserving_flags(&[0xf2, 0xae])
        } else {
            Step::new(&[0xf2, 0xae], flags(10))
                .register(Ecx, 1)
                .register(Edi, 0x6002)
        };
        cases.push(
            Case::from_opaque_flags(format!(
                "REPNE count {count} supplies flags to SETcc and LAHF"
            ))
            .stored_flags(record(0))
            .initial_registers(&[(Eax, 0x1122_007f), (Ebx, 0), (Ecx, count), (Edi, 0x6000)])
            .memory(0x6000, &[1, 0x80, 0x11], ReadOnly)
            .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0x1122_0080))
            .step(scan)
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc3]).register(Ebx, u32::from(count != 0)))
            .step(
                Step::preserving_flags(&[0x9f])
                    .register(Eax, if count == 0 { 0x1122_9280 } else { 0x1122_4680 }),
            ),
        );
    }
    cases
}

test_sequences!(completed_repetition_feeds_successors, successors());
