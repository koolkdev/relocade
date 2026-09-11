use crate::support::cases::Permissions::{ReadOnly, ReadWrite};
use crate::support::sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn sequences() -> Vec<Case> {
    vec![
        Case::preserving_flags("word snapshot survives byte and dword overwrites")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x8877_6655), (Edx, 0xccbb_aa99)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0x34, 0x12]).register(Eax, 0x4433_1234))
            .step(Step::preserving_flags(&[0xb4, 0x56]).register(Eax, 0x4433_5634))
            .step(Step::preserving_flags(&[0x66, 0x89, 0xc1]).register(Ecx, 0x8877_5634))
            .step(Step::preserving_flags(&[0xb0, 0x78]).register(Eax, 0x4433_5678))
            .step(Step::preserving_flags(&[0xb8, 0xaa, 0xbb, 0xcc, 0xdd]).register(Eax, 0xddcc_bbaa))
            .step(Step::preserving_flags(&[0x66, 0x89, 0xca]).register(Edx, 0xccbb_5634)),
        Case::preserving_flags("word address definition forwards the preserved upper half")
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x8000_1234)])
            .memory(0x8000_4000, &[0xa1, 0x88], ReadOnly)
            .step(Step::preserving_flags(&[0x66, 0xbb, 0, 0x40]).register(Ebx, 0x8000_4000))
            .step(Step::preserving_flags(&[0x66, 0x8b, 0x03]).register(Eax, 0x4433_88a1)),
        Case::preserving_flags("word address definition is published before its load faults")
            .initial_registers(&[(Eax, 0x4433_2211), (Ebx, 0x8000_1234)]).backing(0x8000, &[0xa1, 0x88])
            .step(Step::preserving_flags(&[0x66, 0xbb, 0, 0x40]).register(Ebx, 0x8000_4000))
            .step(Step::preserving_flags(&[0x66, 0x8b, 0x03]).fault(0x8000_4000, 0)),
        Case::preserving_flags("word read survives an aliased physical store")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x6000), (Edx, 0xccbb_aa99), (Ebx, 0x4000), (Esi, 0x0123_4567)])
            .map_page(4, 0x8000, ReadWrite).map_page(6, 0x8000, ReadWrite).backing(0x7fff, &[0xa5, 0xa1, 0x88, 0x5a])
            .step(Step::preserving_flags(&[0x66, 0x8b, 0x03]).register(Eax, 0x4433_88a1))
            .step(Step::preserving_flags(&[0x66, 0x89, 0x11]).expect_memory(0x6000, &[0x99, 0xaa]))
            .step(Step::preserving_flags(&[0x66, 0x89, 0xc6]).register(Esi, 0x0123_88a1)),
    ]
}

test_sequences!(aliases_and_publication, sequences());
