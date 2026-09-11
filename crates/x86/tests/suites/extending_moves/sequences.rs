use crate::support::{
    cases::Permissions::{ReadOnly, ReadWrite},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::*;

#[rustfmt::skip]
fn sequences() -> Vec<Case> {
    vec![
        Case::preserving_flags("mixed register aliases read the source before replacing the destination")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x8877_6655), (Edx, 0xccbb_aa99)])
            .step(Step::preserving_flags(&[0xb4, 0x80]).register(Eax, 0x4433_8011))
            .step(Step::preserving_flags(&[0x66, 0x66, 0x0f, 0xbe, 0xc4]).register(Eax, 0x4433_ff80))
            .step(Step::preserving_flags(&[0x0f, 0xb7, 0xd0]).register(Edx, 0x0000_ff80))
            .step(Step::preserving_flags(&[0x0f, 0xbf, 0xc0]).register(Eax, 0xffff_ff80))
            .step(Step::preserving_flags(&[0x0f, 0xb6, 0xcc]).register(Ecx, 0x0000_00ff))
            .step(Step::preserving_flags(&[0xb4, 0x7f]).register(Eax, 0xffff_7f80))
            .step(Step::preserving_flags(&[0x66, 0x0f, 0xb6, 0xc0]).register(Eax, 0xffff_0080))
            .step(Step::preserving_flags(&[0x0f, 0xbf, 0xf0]).register(Esi, 0x0000_0080))
            .step(Step::preserving_flags(&[0x0f, 0xb6, 0xc0]).register(Eax, 0x0000_0080)),
        Case::preserving_flags("loaded values survive alias stores and publish before a later fault")
            .initial_registers(&[(Eax, 0x4433_2211), (Ecx, 0x6000), (Edx, 0xccbb_aa99), (Ebx, 0x4000)])
            .map_page(4, 0x8000, ReadOnly).map_page(6, 0x8000, ReadWrite).backing(0x7fff, &[0x5a, 0x80, 0xff, 0x5a])
            .step(Step::preserving_flags(&[0x0f, 0xb7, 0x03]).register(Eax, 0x0000_ff80))
            .step(Step::preserving_flags(&[0x0f, 0xbe, 0xc0]).register(Eax, 0xffff_ff80))
            .step(Step::preserving_flags(&[0x66, 0x0f, 0xb6, 0xd4]).register(Edx, 0xccbb_00ff))
            .step(Step::preserving_flags(&[0x66, 0x89, 0x11]).expect_memory(0x6000, &[0xff, 0]))
            .step(Step::preserving_flags(&[0x0f, 0xbf, 0xf0]).register(Esi, 0xffff_ff80))
            .step(Step::preserving_flags(&[0x0f, 0xbf, 0x3e]).fault(0xffff_ff80, 0)),
    ]
}

test_sequences!(aliases_and_publication, sequences());
