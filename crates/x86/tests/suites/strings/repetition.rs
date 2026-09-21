//! Repeated transfers retain element ordering and completed progress on faults.

use super::record;
use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn transfers() -> Vec<Case> {
    vec![
        Case::preserving_flags("zero REP MOVSD skips both unmapped operands", &[0xf3, 0xa5]).stored_flags(record(0))
            .initial_registers(&[(Ecx, 0), (Esi, u32::MAX), (Edi, u32::MAX)]),
        Case::preserving_flags("zero REP STOSW skips its unmapped destination", &[0xf3, 0x66, 0xab]).stored_flags(record(1))
            .initial_registers(&[(Ecx, 0), (Edi, u32::MAX)]),
        Case::preserving_flags("REP MOVSB finishes after one element", &[0xf3, 0xa4]).stored_flags(record(0))
            .register(Ecx, 1, 0).register(Esi, 0x4000, 0x4001).register(Edi, 0x7000, 0x7001)
            .memory(0x4000, &[0x12], ReadOnly).memory(0x7000, &[0xa5; 2], ReadWrite).expect_memory(0x7000, &[0x12]),
        Case::preserving_flags("REP MOVSW copies three elements backward", &[0xf3, 0x66, 0xa5]).stored_flags(record(1))
            .register(Ecx, 3, 0).register(Esi, 0x4004, 0x3ffe).register(Edi, 0x7004, 0x6ffe)
            .memory(0x4000, &[1, 2, 3, 4, 5, 6], ReadOnly).memory(0x7000, &[0xa5; 6], ReadWrite)
            .expect_memory(0x7000, &[1, 2, 3, 4, 5, 6]),
        Case::preserving_flags("REP MOVSD copies two complete elements", &[0xf3, 0xa5]).stored_flags(record(0))
            .register(Ecx, 2, 0).register(Esi, 0x4000, 0x4008).register(Edi, 0x7000, 0x7008)
            .memory(0x4000, &[1, 2, 3, 4, 5, 6, 7, 8], ReadOnly).memory(0x7000, &[0xa5; 8], ReadWrite)
            .expect_memory(0x7000, &[1, 2, 3, 4, 5, 6, 7, 8]),
        Case::preserving_flags("REP STOSB repeats AL backward", &[0xf3, 0xaa]).stored_flags(record(1))
            .initial_register(Eax, 0x7856_3412).register(Ecx, 3, 0).register(Edi, 0x7002, 0x6fff)
            .memory(0x7000, &[0xa5; 4], ReadWrite).expect_memory(0x7000, &[0x12; 3]),
        Case::preserving_flags("REP STOSW writes only AX and finishes after one element", &[0xf3, 0x66, 0xab])
            .stored_flags(record(0)).initial_register(Eax, 0x7856_3412).register(Ecx, 1, 0).register(Edi, 0x7000, 0x7002)
            .memory(0x7000, &[0xa5; 4], ReadWrite).expect_memory(0x7000, &[0x12, 0x34]),
        Case::preserving_flags("REP STOSD repeats full EAX", &[0xf3, 0xab]).stored_flags(record(0))
            .initial_register(Eax, 0x7856_3412).register(Ecx, 2, 0).register(Edi, 0x7000, 0x7008)
            .memory(0x7000, &[0xa5; 8], ReadWrite).expect_memory(0x7000, &[0x12, 0x34, 0x56, 0x78, 0x12, 0x34, 0x56, 0x78]),
        Case::preserving_flags("REP STOSD finishes after wrapping EDI without accessing page zero", &[0xf3, 0xab])
            .stored_flags(record(0)).initial_register(Eax, 0x7856_3412).register(Ecx, 1, 0).register(Edi, 0xffff_fffc, 0)
            .memory(0xffff_fffc, &[0xa5; 4], ReadWrite).expect_memory(0xffff_fffc, &[0x12, 0x34, 0x56, 0x78]),
        Case::preserving_flags("REP MOVSB finishes after ESI underflow without another source read", &[0xf3, 0xa4])
            .stored_flags(record(1)).register(Ecx, 1, 0).register(Esi, 0, u32::MAX).register(Edi, 0x7000, 0x6fff)
            .memory(0, &[0x12], ReadOnly).memory(0x7000, &[0xa5], ReadWrite).expect_memory(0x7000, &[0x12]),
    ]
}

#[rustfmt::skip]
fn overlap() -> Vec<Case> {
    vec![
        Case::preserving_flags("REP MOVSB forward overlap propagates the prior write", &[0xf3, 0xa4])
            .stored_flags(record(0xfe)).register(Ecx, 4, 0).register(Esi, 0x4000, 0x4004).register(Edi, 0x4001, 0x4005)
            .memory(0x4000, &[1, 2, 3, 4, 5, 6], ReadWrite).expect_memory(0x4000, &[1, 1, 1, 1, 1, 6]),
        Case::preserving_flags("REP MOVSB backward overlap propagates the prior write", &[0xf3, 0xa4])
            .stored_flags(record(0xff)).register(Ecx, 4, 0).register(Esi, 0x4004, 0x4000).register(Edi, 0x4003, 0x3fff)
            .memory(0x4000, &[1, 2, 3, 4, 5, 6], ReadWrite).expect_memory(0x4000, &[5, 5, 5, 5, 5, 6]),
        Case::preserving_flags("REP MOVSD captures each operand but sees preceding overlapping stores", &[0xf3, 0xa5])
            .stored_flags(record(0xfe)).register(Ecx, 2, 0).register(Esi, 0x4000, 0x4008).register(Edi, 0x4001, 0x4009)
            .memory(0x4000, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], ReadWrite)
            .expect_memory(0x4000, &[1, 1, 2, 3, 4, 4, 6, 7, 8, 10]),
        Case::preserving_flags("REP MOVSB physical aliases carry writes into the next read", &[0xf3, 0xa4])
            .stored_flags(record(0xfe)).register(Ecx, 4, 0).register(Esi, 0x4000, 0x4004).register(Edi, 0x7001, 0x7005)
            .map_page(4, 0x8000, ReadOnly).map_page(7, 0x8000, ReadWrite).backing(0x8000, &[1, 2, 3, 4, 5, 6])
            .expect_memory(0x7001, &[1, 1, 1, 1]),
    ]
}

#[rustfmt::skip]
fn partial_faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("REP MOVSD source fault precedes a missing destination after progress", &[0xf3, 0xa5])
            .stored_flags(record(0)).register(Ecx, 3, 1).register(Esi, 0x4ff8, 0x5000).register(Edi, 0x7ff8, 0x8000)
            .memory(0x4ff8, &[1, 2, 3, 4, 5, 6, 7, 8], ReadOnly).memory(0x7ff8, &[0xa5; 8], ReadWrite)
            .expect_memory(0x7ff8, &[1, 2, 3, 4, 5, 6, 7, 8]).fault(0x5000, 0),
        Case::preserving_flags("REP STOSD retains two stores before a destination fault", &[0xf3, 0xab])
            .stored_flags(record(0)).initial_register(Eax, 0x1234_5678).register(Ecx, 4, 2).register(Edi, 0x7ff8, 0x8000)
            .memory(0x7ff8, &[0xa5; 8], ReadWrite).expect_memory(0x7ff8, &[0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12])
            .fault(0x8000, 2),
        Case::preserving_flags("REP MOVSW keeps one completed iteration and no partial second write", &[0xf3, 0x66, 0xa5])
            .stored_flags(record(0xfe)).register(Ecx, 3, 2).register(Esi, 0x4000, 0x4002).register(Edi, 0x7ffd, 0x7fff)
            .memory(0x4000, &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc], ReadOnly)
            .memory(0x7ffd, &[0xa5, 0xa5, 0xa5], ReadWrite).expect_memory(0x7ffd, &[0x12, 0x34, 0xa5]).fault(0x8000, 2),
        Case::preserving_flags("REP MOVSW uses full ECX despite word operand override", &[0xf3, 0x66, 0xa5])
            .stored_flags(record(0xfe)).register(Ecx, 0x10000, 0xffff).register(Esi, 0x4ffe, 0x5000)
            .register(Edi, 0x7000, 0x7002).memory(0x4ffe, &[0x12, 0x34], ReadOnly).memory(0x7000, &[0xa5; 4], ReadWrite)
            .expect_memory(0x7000, &[0x12, 0x34]).fault(0x5000, 0),
        Case::preserving_flags("REP MOVSB backward fault leaves current indices and remaining count", &[0xf3, 0xa4])
            .stored_flags(record(0xff)).register(Ecx, 3, 1).register(Esi, 0x4001, 0x3fff).register(Edi, 0x7001, 0x6fff)
            .memory(0x4000, &[0x12, 0x34], ReadOnly).memory(0x7000, &[0xa5; 2], ReadWrite)
            .expect_memory(0x7000, &[0x12, 0x34]).fault(0x3fff, 0),
    ]
}

#[rustfmt::skip]
fn successors() -> Vec<Sequence> {
    vec![
        Sequence::preserving_flags("address-sized REP results feed full-register aliases and memory reads")
            .stored_flags(record(0)).initial_registers(&[(Esi, 0xeeee_4000), (Edi, 0xdddd_6000)])
            .memory(0x4000, &[0x11, 0x22], ReadOnly).memory(0x6000, &[0xaa; 4], ReadWrite)
            .step(Step::preserving_flags(&[0xb9, 2, 0, 0xcd, 0xab]).register(Ecx, 0xabcd_0002))
            .step(Step::preserving_flags(&[0x67, 0xf3, 0xa4])
                .register(Ecx, 0xabcd_0000).register(Esi, 0xeeee_4002).register(Edi, 0xdddd_6002).expect_memory(0x6000, &[0x11, 0x22]))
            .step(Step::preserving_flags(&[0x89, 0xcb]).register(Ebx, 0xabcd_0000))
            .step(Step::preserving_flags(&[0x89, 0xf0]).register(Eax, 0xeeee_4002))
            .step(Step::preserving_flags(&[0x89, 0xf9]).register(Ecx, 0xdddd_6002))
            .step(Step::preserving_flags(&[0xa1, 0, 0x60, 0, 0]).register(Eax, 0xaaaa_2211)),
        Sequence::preserving_flags("a REP element fault prevents its compiled successor store")
            .stored_flags(record(0)).initial_registers(&[(Ecx, 2), (Esi, 0x4fff), (Edi, 0x6000)])
            .map_page(4, 0x8000, ReadOnly).backing(0x8fff, &[0x11]).memory(0x6000, &[0xaa; 2], ReadWrite)
            .step(Step::preserving_flags(&[0xf3, 0xa4])
                .register(Ecx, 1).register(Esi, 0x5000).register(Edi, 0x6001).expect_memory(0x6000, &[0x11]).fault(0x5000, 0))
            .trailing_code(&[0xc6, 0x05, 0, 0x60, 0, 0, 0x99], 1),
        Sequence::preserving_flags("REP STOSW prefix state resets before the following STOSD")
            .stored_flags(record(0)).initial_registers(&[(Eax, 0x7856_3412), (Ecx, 2), (Edi, 0x6000)])
            .memory(0x6000, &[0xa5; 8], ReadWrite)
            .step(Step::preserving_flags(&[0x66, 0xf3, 0xab]).register(Ecx, 0).register(Edi, 0x6004)
                .expect_memory(0x6000, &[0x12, 0x34, 0x12, 0x34]))
            .step(Step::preserving_flags(&[0xab]).register(Edi, 0x6008).expect_memory(0x6004, &[0x12, 0x34, 0x56, 0x78])),
        Sequence::preserving_flags("AX AH and CX writes feed a REP STOSW progress fault")
            .stored_flags(record(0)).initial_registers(&[(Eax, 0xaabb_ccdd), (Ecx, 0x0001_00aa), (Edi, 0x7ffc)])
            .memory(0x7ffc, &[0xa5; 4], ReadWrite)
            .step(Step::preserving_flags(&[0x66, 0xb8, 0x12, 0x34]).register(Eax, 0xaabb_3412))
            .step(Step::preserving_flags(&[0xb4, 0x56]).register(Eax, 0xaabb_5612))
            .step(Step::preserving_flags(&[0x66, 0xb9, 3, 0]).register(Ecx, 0x0001_0003))
            .step(Step::preserving_flags(&[0xf3, 0x66, 0xab]).register(Ecx, 0x0001_0001).register(Edi, 0x8000)
                .expect_memory(0x7ffc, &[0x12, 0x56, 0x12, 0x56]).fault(0x8000, 2)),
    ]
}

test_cases!(repeat_counts_widths_and_direction, transfers());
test_cases!(sequential_overlapping_elements, overlap());
test_cases!(faults_retain_completed_elements, partial_faults());
test_sequences!(repeat_state_and_successors, successors());
