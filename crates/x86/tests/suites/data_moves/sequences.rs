use crate::support::{
    cases::Permissions::{ReadOnly, ReadWrite},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn histories() -> Vec<Sequence> {
    vec![
        Sequence::preserving_flags("word snapshot survives byte and dword overwrites")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x8877_6655), (Edx, 0xccbb_aa99)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0x34, 0x12]).register(Eax, 0x4433_1234))
            .step(Step::preserving_flags(&[0xb4, 0x56]).register(Eax, 0x4433_5634))
            .step(Step::preserving_flags(&[0x66, 0x89, 0xc1]).register(Ecx, 0x8877_5634))
            .step(Step::preserving_flags(&[0xb0, 0x78]).register(Eax, 0x4433_5678))
            .step(Step::preserving_flags(&[0xb8, 0xaa, 0xbb, 0xcc, 0xdd]).register(Eax, 0xddcc_bbaa))
            .step(Step::preserving_flags(&[0x66, 0x89, 0xca]).register(Edx, 0xccbb_5634)),
        Sequence::preserving_flags("word address definition forwards the preserved upper half")
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x8000_1234)]).memory(0x8000_4000, &[0xa1, 0x88], ReadOnly)
            .step(Step::preserving_flags(&[0x66, 0xbb, 0, 0x40]).register(Ebx, 0x8000_4000))
            .step(Step::preserving_flags(&[0x66, 0x8b, 0x03]).register(Eax, 0x4433_88a1)),
        Sequence::preserving_flags("word read survives an aliased physical store")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x6000), (Edx, 0xccbb_aa99), (Ebx, 0x4000), (Esi, 0x0123_4567)])
            .map_page(4, 0x8000, ReadWrite).map_page(6, 0x8000, ReadWrite).backing(0x7fff, &[0xa5, 0xa1, 0x88, 0x5a])
            .step(Step::preserving_flags(&[0x66, 0x8b, 0x03]).register(Eax, 0x4433_88a1))
            .step(Step::preserving_flags(&[0x66, 0x89, 0x11]).expect_memory(0x6000, &[0x99, 0xaa]))
            .step(Step::preserving_flags(&[0x66, 0x89, 0xc6]).register(Esi, 0x0123_88a1)),
        Sequence::preserving_flags("partial definition feeds address and full-register source")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x5000), (Ebx, 0x4000)]).map_page(4, 0x8000, ReadWrite)
            .map_page(5, 0x9000, ReadWrite).backing(0x801f, &[0xa5, 0x80, 0x5a])
            .backing(0x8fff, &[0xa5, 0, 0, 0, 0, 0x5a])
            .step(Step::preserving_flags(&[0xb3, 0x20]).register(Ebx, 0x4020))
            .step(Step::preserving_flags(&[0x8a, 0x23]).register(Eax, 0x4433_8011))
            .step(Step::preserving_flags(&[0x89, 0x01]).expect_memory(0x5000, &[0x11, 0x80, 0x33, 0x44])),
        Sequence::preserving_flags("data fault publishes only the completed byte definition")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x5000), (Ebx, 0x4000)]).backing(0x801f, &[0xa5, 0x80, 0x5a])
            .backing(0x8fff, &[0xa5, 0, 0, 0, 0, 0x5a])
            .step(Step::preserving_flags(&[0xb3, 0x20]).register(Ebx, 0x4020))
            .step(Step::preserving_flags(&[0x8a, 0x23]).fault(0x4020, 0)).trailing_code(&[0x89, 0x01], 1),
        Sequence::preserving_flags("prior register and byte store survive a later fault")
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x10ff_eedd)]).map_page(4, 0x8000, ReadWrite)
            .backing(0x7fff, &[0xa5, 0xcc, 0x5a]).backing(0x9000, &[0xa0, 0x66, 0xc7, 0x88])
            .step(Step::preserving_flags(&[0xc7, 0xc3, 0, 0x40, 0, 0]).register(Ebx, 0x4000))
            .step(Step::preserving_flags(&[0xc6, 0x03, 0x80]).expect_memory(0x4000, &[0x80]))
            .step(Step::preserving_flags(&[0xa1, 0, 0x50, 0, 0]).fault(0x5000, 0)),
        Sequence::preserving_flags("accumulator byte and dword views share their stored value")
            .initial_register(Eax, 0x4433_2211).map_page(4, 0x8000, ReadWrite)
            .backing(0x7fff, &[0xa5, 0, 0, 0, 0, 0x5a])
            .step(Step::preserving_flags(&[0xc6, 0xc4, 0x80]).register(Eax, 0x4433_8011))
            .step(Step::preserving_flags(&[0xa3, 0, 0x40, 0, 0]).expect_memory(0x4000, &[0x11, 0x80, 0x33, 0x44]))
            .step(Step::preserving_flags(&[0xc7, 0xc0, 0xef, 0xbe, 0xad, 0xde]).register(Eax, 0xdead_beef))
            .step(Step::preserving_flags(&[0xa0, 1, 0x40, 0, 0]).register(Eax, 0xdead_be80)),
        Sequence::preserving_flags("a dword copy keeps its value after the source is replaced")
            .initial_registers(&[(Eax, 0x1111_1111), (Ecx, 0x2222_2222), (Edx, 0x3333_3333)])
            .step(Step::preserving_flags(&[0x89, 0xc1]).register(Ecx, 0x1111_1111))
            .step(Step::preserving_flags(&[0xb8, 9, 0, 0, 0]).register(Eax, 9))
            .step(Step::preserving_flags(&[0x8b, 0xd1]).register(Edx, 0x1111_1111)),
    ]
}

test_sequences!(value_dependencies_and_fault_publication, histories());
