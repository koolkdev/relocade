use crate::support::{
    cases::Permissions::{ReadOnly, ReadWrite},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn sequences() -> Vec<Case> {
    vec![
        Case::preserving_flags("prior register and byte store survive a later fault")
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x10ff_eedd)])
            .map_page(4, 0x8000, ReadWrite).backing(0x7fff, &[0xa5, 0xcc, 0x5a]).backing(0x9000, &[0xa0, 0x66, 0xc7, 0x88])
            .step(Step::preserving_flags(&[0xc7, 0xc3, 0, 0x40, 0, 0]).register(Ebx, 0x4000))
            .step(Step::preserving_flags(&[0xc6, 0x03, 0x80]).expect_memory(0x4000, &[0x80]))
            .step(Step::preserving_flags(&[0xa1, 0, 0x50, 0, 0]).fault(0x5000, 0)),
        Case::preserving_flags("forwarded address continues through an absolute load")
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x10ff_eedd)])
            .map_page(4, 0x8000, ReadWrite).map_page(5, 0x9000, ReadOnly)
            .backing(0x7fff, &[0xa5, 0xcc, 0x5a]).backing(0x9000, &[0xa0, 0x66, 0xc7, 0x88])
            .step(Step::preserving_flags(&[0xc7, 0xc3, 0, 0x40, 0, 0]).register(Ebx, 0x4000))
            .step(Step::preserving_flags(&[0xc6, 0x03, 0x80]).expect_memory(0x4000, &[0x80]))
            .step(Step::preserving_flags(&[0xa1, 0, 0x50, 0, 0]).register(Eax, 0x88c7_66a0)),
        Case::preserving_flags("accumulator byte and dword views share their stored value")
            .initial_register(Eax, 0x4433_2211).map_page(4, 0x8000, ReadWrite).backing(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a])
            .step(Step::preserving_flags(&[0xc6, 0xc4, 0x80]).register(Eax, 0x4433_8011))
            .step(Step::preserving_flags(&[0xa3, 0, 0x40, 0, 0]).expect_memory(0x4000, &[0x11, 0x80, 0x33, 0x44]))
            .step(Step::preserving_flags(&[0xc7, 0xc0, 0xef, 0xbe, 0xad, 0xde]).register(Eax, 0xdead_beef))
            .step(Step::preserving_flags(&[0xa0, 1, 0x40, 0, 0]).register(Eax, 0xdead_be80)),
        Case::preserving_flags("held absolute load survives a grouped store through a physical alias")
            .initial_register(Eax, 0x4433_2211).map_page(4, 0x8000, ReadWrite).map_page(6, 0x8000, ReadWrite)
            .backing(0x7fff, &[0xa5, 0x11, 0x22, 0x33, 0x44, 0x5a])
            .step(Step::preserving_flags(&[0xa1, 0, 0x40, 0, 0]).register(Eax, 0x4433_2211))
            .step(Step::preserving_flags(&[0xc7, 0x05, 0, 0x60, 0, 0, 0x99, 0x77, 0x66, 0x55]).expect_memory(0x6000, &[0x99, 0x77, 0x66, 0x55]))
            .step(Step::preserving_flags(&[0x89, 0xc6]).register(Esi, 0x4433_2211)),
    ]
}

test_sequences!(progress_and_aliases, sequences());
